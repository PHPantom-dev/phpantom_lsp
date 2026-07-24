use super::*;
use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::completion::resolver::VarResolutionCtx;
use crate::completion::types::narrowing;
use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, ShapeEntry};
use crate::types::ResolvedType;

// ─── Statement processing ───────────────────────────────────────────────────

/// Process a single statement, updating `scope` with any variable
/// assignments, narrowing, or control-flow effects.
pub(crate) fn process_statement<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
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
        Statement::Unset(unset_stmt) => {
            for val in unset_stmt.values.iter() {
                if let Expression::Variable(Variable::Direct(dv)) = val {
                    scope.remove(bytes_to_str(dv.name));
                }
            }
        }
        Statement::Namespace(ns) => {
            walk_body_forward(ns.statements().iter(), scope, ctx);
        }
        Statement::Global(global) => {
            for var in global.variables.iter() {
                if let Variable::Direct(dv) = var {
                    let var_name = bytes_to_str(dv.name).to_string();
                    if let Some(top_scope) = &ctx.top_level_scope {
                        if let Some(types) = top_scope.get(&atom(&var_name)) {
                            scope.set(&var_name, types.clone());
                        } else {
                            scope.set_empty(&var_name);
                        }
                    } else {
                        scope.set_empty(&var_name);
                    }
                }
            }
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                process_assignment_expr(val, scope, ctx);

                // Record `&&` chain snapshots so that member accesses
                // after an instanceof/null guard see the narrowed type.
                // E.g. `return $x instanceof Foo && $x->bar()`
                record_and_chain_snapshots(val, scope, ctx);
                record_or_chain_snapshots(val, scope, ctx);

                // Record narrowed snapshots inside match(true) arms
                // and ternary instanceof branches.
                if is_diagnostic_scope_active() {
                    record_match_ternary_snapshots(val, scope, ctx);
                }
            }
        }
        _ => {}
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
    let expr = expr_stmt.expression;

    // Try inline `/** @var Type $x */` override first.
    match try_process_inline_var_override(expr, stmt_offset(expr), scope, ctx) {
        VarOverrideResult::NamedVar => {
            // Re-record the scope snapshot at this expression's offset
            // so that variable lookups within the same statement (e.g.
            // `$app` in `$client = $app->make(...)` where a preceding
            // `@var` block declared `$app`) see the updated types.
            // The snapshot recorded by `walk_body_for_diagnostics` at
            // the statement start was taken *before* the `@var`
            // override was applied.
            record_scope_snapshot(stmt_offset(expr), scope);
            return;
        }
        VarOverrideResult::NoVar => {
            // A `@var Type` (no variable name) was applied to the
            // assignment LHS.  The override already set the LHS type,
            // so skip further assignment processing to avoid the RHS
            // overwriting the docblock type.
            return;
        }
        VarOverrideResult::None => {}
    }

    // Record intermediate scope snapshots within `&&` chains so that
    // member accesses after an instanceof/null guard see the narrowed
    // type.  E.g. `$x instanceof Foo && $x->bar()` as an expression
    // statement.
    record_and_chain_snapshots(expr, scope, ctx);
    record_or_chain_snapshots(expr, scope, ctx);

    // Record narrowed snapshots inside match(true) arms and ternary
    // instanceof branches within this expression.
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(expr, scope, ctx);
    }

    // Process assignments.
    process_assignment_expr(expr, scope, ctx);

    process_by_ref_closure_captures(expr, scope, ctx);

    // Process pass-by-reference parameter type inference.
    process_pass_by_ref(expr, scope, ctx);

    // Process assert narrowing.
    process_assert_narrowing(expr, scope, ctx);

    // Process increment/decrement: $a++, ++$a, $a--, --$a.
    process_increment_decrement(expr, scope, ctx);
}

pub(crate) fn process_by_ref_closure_captures<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            let Some(func_name) = (match fc.function {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                _ => None,
            }) else {
                return;
            };

            let mut next_positional = 0usize;
            for arg in fc.argument_list.arguments.iter() {
                let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
                if let Expression::Closure(closure) = arg_expr
                    && function_invokes_callable_arg_immediately(&func_name, &selector, ctx)
                {
                    process_by_ref_closure_capture(closure, scope, ctx);
                }
            }
        }
        Expression::Call(Call::Method(mc)) => {
            let Some(method_name) = (match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => {
                    Some(bytes_to_str(ident.value).to_string())
                }
                _ => None,
            }) else {
                return;
            };
            let receiver_names = receiver_class_names(mc.object, scope, ctx);
            if receiver_names.is_empty() {
                return;
            }

            let mut next_positional = 0usize;
            for arg in mc.argument_list.arguments.iter() {
                let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
                if let Expression::Closure(closure) = arg_expr
                    && method_invokes_callable_arg_immediately(
                        &receiver_names,
                        &method_name,
                        &selector,
                        ctx,
                    )
                {
                    process_by_ref_closure_capture(closure, scope, ctx);
                }
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            let Some(method_name) = (match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => {
                    Some(bytes_to_str(ident.value).to_string())
                }
                _ => None,
            }) else {
                return;
            };
            let receiver_names = receiver_class_names(mc.object, scope, ctx);
            if receiver_names.is_empty() {
                return;
            }

            let mut next_positional = 0usize;
            for arg in mc.argument_list.arguments.iter() {
                let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
                if let Expression::Closure(closure) = arg_expr
                    && method_invokes_callable_arg_immediately(
                        &receiver_names,
                        &method_name,
                        &selector,
                        ctx,
                    )
                {
                    process_by_ref_closure_capture(closure, scope, ctx);
                }
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let Some(method_name) = (match &sc.method {
                ClassLikeMemberSelector::Identifier(ident) => {
                    Some(bytes_to_str(ident.value).to_string())
                }
                _ => None,
            }) else {
                return;
            };
            let receiver_names = static_receiver_class_names(sc.class, ctx);
            if receiver_names.is_empty() {
                return;
            }

            let mut next_positional = 0usize;
            for arg in sc.argument_list.arguments.iter() {
                let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
                if let Expression::Closure(closure) = arg_expr
                    && method_invokes_callable_arg_immediately(
                        &receiver_names,
                        &method_name,
                        &selector,
                        ctx,
                    )
                {
                    process_by_ref_closure_capture(closure, scope, ctx);
                }
            }
        }
        Expression::Parenthesized(inner) => {
            process_by_ref_closure_captures(inner.expression, scope, ctx);
        }
        Expression::Assignment(assignment) => {
            process_by_ref_closure_captures(assignment.rhs, scope, ctx);
        }
        _ => {}
    }
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

/// Extract a call argument's value expression and the selector that
/// identifies which parameter it fills. `next_positional` tracks the
/// running position of positional arguments (PHP requires positional
/// arguments to precede named ones, so this stays aligned with the
/// parameter list).
pub(crate) fn arg_expr_and_selector<'b>(
    arg: &'b Argument<'b>,
    next_positional: &mut usize,
) -> (&'b Expression<'b>, ArgSelector) {
    match arg {
        Argument::Positional(pos) => {
            let selector = ArgSelector::Position(*next_positional);
            *next_positional += 1;
            (pos.value, selector)
        }
        Argument::Named(named) => (
            named.value,
            ArgSelector::Name(bytes_to_str(named.name.value).to_string()),
        ),
    }
}

/// Find the callee parameter that a call argument fills, honouring both
/// positional and named binding.
pub(crate) fn select_param<'p>(
    parameters: impl Iterator<Item = &'p FunctionLikeParameter<'p>>,
    selector: &ArgSelector,
) -> Option<&'p FunctionLikeParameter<'p>> {
    match selector {
        ArgSelector::Position(idx) => parameters.into_iter().nth(*idx),
        ArgSelector::Name(name) => parameters
            .into_iter()
            .find(|param| bytes_to_str(param.variable.name).trim_start_matches('$') == name),
    }
}

pub(crate) fn function_invokes_callable_arg_immediately(
    func_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    with_parsed_program(
        ctx.content,
        "function_invokes_callable_arg",
        |program, _| {
            let mut stmts = Vec::new();
            flatten_namespaced_statements(program.statements.iter(), &mut stmts);
            stmts.into_iter().any(|stmt| {
                if let Statement::Function(func) = stmt
                    && bytes_to_str(func.name.value).eq_ignore_ascii_case(func_name)
                {
                    let Some(param) = select_param(func.parameter_list.parameters.iter(), selector)
                    else {
                        return false;
                    };
                    return !function_param_has_invocation_tag(
                        func.name.span.start.offset as usize,
                        ctx.content,
                        bytes_to_str(param.variable.name),
                        "param-later-invoked-callable",
                    );
                }
                false
            })
        },
    )
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

pub(crate) fn receiver_class_names(
    expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name);
            if var_name == "$this" && !ctx.current_class.name.is_empty() {
                return vec![
                    ctx.current_class.name.to_string(),
                    ctx.current_class.fqn().to_string(),
                ];
            }
            scope
                .get(var_name)
                .iter()
                .filter_map(|rt| rt.class_info.as_ref())
                .flat_map(|cls| [cls.name.to_string(), cls.fqn().to_string()])
                .collect()
        }
        Expression::Parenthesized(inner) => receiver_class_names(inner.expression, scope, ctx),
        _ => Vec::new(),
    }
}

pub(crate) fn static_receiver_class_names(
    expr: &Expression<'_>,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    match expr {
        Expression::Self_(_) | Expression::Static(_) if !ctx.current_class.name.is_empty() => {
            vec![
                ctx.current_class.name.to_string(),
                ctx.current_class.fqn().to_string(),
            ]
        }
        Expression::Parent(_) => ctx
            .current_class
            .parent_class
            .map(|name| vec![name.to_string()])
            .unwrap_or_default(),
        Expression::Identifier(ident) => vec![bytes_to_str(ident.value()).to_string()],
        Expression::Parenthesized(inner) => static_receiver_class_names(inner.expression, ctx),
        _ => Vec::new(),
    }
}

pub(crate) fn method_invokes_callable_arg_immediately(
    receiver_names: &[String],
    method_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    with_parsed_program(ctx.content, "method_invokes_callable_arg", |program, _| {
        let mut stmts = Vec::new();
        flatten_namespaced_statements(program.statements.iter(), &mut stmts);
        stmts.into_iter().any(|stmt| {
            let members = match stmt {
                Statement::Class(class)
                    if class_name_matches_receiver(class.name.value, receiver_names) =>
                {
                    Some(class.members.iter())
                }
                _ => None,
            };

            let Some(members) = members else {
                return false;
            };

            members.into_iter().any(|member| {
                if let ClassLikeMember::Method(method) = member
                    && bytes_to_str(method.name.value).eq_ignore_ascii_case(method_name)
                {
                    let Some(param) =
                        select_param(method.parameter_list.parameters.iter(), selector)
                    else {
                        return false;
                    };
                    return node_param_has_invocation_tag(
                        method.name.span.start.offset as usize,
                        ctx.content,
                        bytes_to_str(param.variable.name),
                        "param-immediately-invoked-callable",
                    );
                }
                false
            })
        })
    })
}

pub(crate) fn class_name_matches_receiver(name: &[u8], receiver_names: &[String]) -> bool {
    let class_name = bytes_to_str(name);
    receiver_names.iter().any(|receiver| {
        receiver.eq_ignore_ascii_case(class_name)
            || crate::util::short_name(receiver).eq_ignore_ascii_case(class_name)
    })
}

pub(crate) fn function_param_has_invocation_tag(
    node_start: usize,
    content: &str,
    param_name: &str,
    tag_name: &str,
) -> bool {
    node_param_has_invocation_tag(node_start, content, param_name, tag_name)
}

pub(crate) fn node_param_has_invocation_tag(
    node_start: usize,
    content: &str,
    param_name: &str,
    tag_name: &str,
) -> bool {
    let Some(docblock) = preceding_docblock_text(content, node_start) else {
        return false;
    };
    docblock.lines().any(|line| {
        let line = line
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches('*')
            .trim_end_matches("*/")
            .trim();
        line.starts_with(&format!("@{tag_name}"))
            && line
                .split_whitespace()
                .any(|part| part.trim_matches(',') == param_name)
    })
}

pub(crate) fn preceding_docblock_text(content: &str, node_start: usize) -> Option<&str> {
    let before = content.get(..node_start)?;
    let doc_end = before.rfind("*/")? + 2;
    let between = &before[doc_end..];
    if between.contains(';') || between.contains('{') || between.contains('}') {
        return None;
    }
    let doc_start = before[..doc_end].rfind("/**")?;
    Some(&before[doc_start..doc_end])
}

pub(crate) fn process_by_ref_closure_capture<'b>(
    closure: &'b Closure<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let captured: Vec<String> = closure
        .use_clause
        .as_ref()
        .map(|use_clause| {
            use_clause
                .variables
                .iter()
                .filter(|use_var| use_var.ampersand.is_some())
                .map(|use_var| bytes_to_str(use_var.variable.name).to_string())
                .collect()
        })
        .unwrap_or_default();
    if captured.is_empty() {
        return;
    }

    let full_ctx = ctx.with_cursor_offset(u32::MAX);
    let mut closure_scope = ScopeState::new();

    let this_types = scope.get("$this");
    if !this_types.is_empty() {
        closure_scope.set("$this", this_types.to_vec());
    }

    if let Some(ref use_clause) = closure.use_clause {
        for use_var in use_clause.variables.iter() {
            let var_name = bytes_to_str(use_var.variable.name).to_string();
            let from_outer = scope.get(&var_name);
            if !from_outer.is_empty() {
                closure_scope.set(&var_name, from_outer.to_vec());
            } else if scope.contains(&var_name) {
                closure_scope.set_empty(&var_name);
            }
        }
    }

    seed_closure_params(
        &mut closure_scope,
        &closure.parameter_list,
        closure.span().start.offset,
        &[],
        &full_ctx,
    );

    walk_body_forward(
        closure.body.statements.iter(),
        &mut closure_scope,
        &full_ctx,
    );

    for var_name in captured {
        scope.invalidate_dependent_keys(&var_name);
        let types = closure_scope.get(&var_name).to_vec();
        if !types.is_empty() {
            scope.set(&var_name, types);
        } else if closure_scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
    }
}

/// Process increment/decrement expressions (`$a++`, `++$a`, `$a--`, `--$a`).
///
/// For numeric types (int, float), the type is preserved.
/// For numeric strings, the result becomes `int|float`.
/// For general strings, PHP increments alphabetically (stays string).
pub(crate) fn process_increment_decrement<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    _ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::unary::{UnaryPostfixOperator, UnaryPrefixOperator};

    let var_expr = match expr {
        Expression::UnaryPostfix(postfix) => match &postfix.operator {
            UnaryPostfixOperator::PostIncrement(_) | UnaryPostfixOperator::PostDecrement(_) => {
                postfix.operand
            }
        },
        Expression::UnaryPrefix(prefix) => match &prefix.operator {
            UnaryPrefixOperator::PreIncrement(_) | UnaryPrefixOperator::PreDecrement(_) => {
                prefix.operand
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

    // Check if the type is numeric or a numeric-string (including
    // literal string values like '123').  If so, increment produces
    // int|float because PHP converts numeric strings to numbers.
    let current_type = ResolvedType::types_joined(&existing);
    let is_numeric_like = {
        let lower = current_type.to_string().to_ascii_lowercase();
        lower == "numeric" || lower == "numeric-string"
    } || current_type.is_subtype_of(&PhpType::Named("numeric-string".into()));
    if is_numeric_like {
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::Union(vec![
                PhpType::int(),
                PhpType::float(),
            ]))],
        );
    } else if current_type.is_string_literal() {
        // Non-numeric string literal: PHP increments alphabetically
        // (e.g. "a" → "b"), so the result is still a string but no
        // longer the same literal value.  Widen to `string`.
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::string())],
        );
    }
    // For int, float, plain string: the type stays the same
    // (PHP preserves the type for numeric increment/decrement).
}

/// Get the byte offset of an expression (used for cursor comparisons).
pub(crate) fn stmt_offset(expr: &Expression<'_>) -> u32 {
    expr.span().start.offset
}

/// Try to process an inline `/** @var Type $x */` docblock override.
///
/// Returns `true` if an override was found and applied.
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

pub(crate) fn try_process_inline_var_override<'b>(
    expr: &'b Expression<'b>,
    expr_offset: u32,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> VarOverrideResult {
    // Parse the inline @var docblock at this expression's position.
    let offset = expr_offset as usize;
    if offset == 0 {
        return VarOverrideResult::None;
    }

    // Look for `/** @var Type $varName */` before this expression.
    let before = &ctx.content[..offset.min(ctx.content.len())];
    let trimmed = before.trim_end();

    // Quick check: does it end with `*/`?
    if !trimmed.ends_with("*/") {
        return VarOverrideResult::None;
    }

    // Find the docblock start.
    let doc_end = trimmed.len();
    let doc_start = if let Some(pos) = trimmed.rfind("/**") {
        pos
    } else {
        return VarOverrideResult::None;
    };

    let doc_text = &trimmed[doc_start..doc_end];

    // Try multi-@var first: a single docblock may declare several
    // variables (e.g. `/** @var App $app  @var array{…} $params */`).
    let multi = parse_all_inline_var_docblocks(doc_text, ctx);
    if !multi.is_empty() {
        // When the cursor is inside the RHS of an assignment, skip
        // overriding the LHS variable so that hover/completion on the
        // RHS sees the pre-override type.  E.g.:
        //   /** @var array<string, mixed> $response */
        //   $response = $response->json();
        // Hovering on the RHS `$response` should show `ApiResponse`,
        // not `array<string, mixed>`.
        let skip_var: Option<String> = if let Expression::Assignment(assignment) = expr {
            let rhs_span = assignment.rhs.span();
            let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                && ctx.cursor_offset <= rhs_span.end.offset;
            if cursor_in_rhs {
                if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    Some(bytes_to_str(dv.name).to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        for (var_name, php_type) in &multi {
            if skip_var.as_deref() == Some(var_name.as_str()) {
                continue;
            }
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
        }
        // After processing the immediate docblock, scan backwards for
        // additional standalone docblocks that precede it.  This handles
        // patterns like:
        //   /** @var App $app  @var array{…} $params */
        //   /** @var Client $client */
        //   $client = $app->make(Client::class);
        // where the first block is separated from the expression by
        // another docblock.
        apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);

        // When the @var variable names all differ from the assignment
        // LHS, return None so the caller continues processing the
        // assignment.  E.g.:
        //   /** @var Foo[] $items */
        //   $item = array_shift($items);
        // The @var sets `$items` in scope (done above), and the caller
        // must also process `$item = array_shift($items)`.
        //
        // When any @var name matches the LHS, return NamedVar so the
        // caller skips the assignment (the @var type is authoritative).
        if let Expression::Assignment(assignment) = expr
            && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
        {
            let lhs_name = bytes_to_str(dv.name).to_string();
            if !multi.iter().any(|(n, _)| *n == lhs_name) {
                return VarOverrideResult::None;
            }
        }
        return VarOverrideResult::NamedVar;
    }

    // Also check for `/** @var Type */` without variable name — this
    // applies to the immediately following expression if it's a simple
    // variable or assignment.
    if let Some(php_type) = parse_inline_var_docblock_no_var(doc_text, ctx) {
        let resolved = resolve_type_to_resolved_types(&php_type, ctx);
        if let Expression::Assignment(assignment) = expr {
            if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                // When the cursor is inside the RHS, skip the override
                // so that the variable retains its pre-assignment type.
                // E.g. `/** @var array<string, mixed> */ $data = $data->toArray()`
                // — the cursor on `$data->` in the RHS should see Data, not array.
                let rhs_span = assignment.rhs.span();
                let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                    && ctx.cursor_offset <= rhs_span.end.offset;
                if cursor_in_rhs {
                    return VarOverrideResult::None;
                }

                // Scalar-blocking: when the RHS resolves to a concrete
                // scalar type (string, int, bool, etc.), reject a class
                // `@var` override.  E.g. `/** @var Session */ $s =
                // $this->getName()` where `getName()` returns `string`
                // should NOT override `$s` to `Session`.
                let native_type = resolve_rhs_native_type(assignment.rhs, scope, ctx);
                if let Some(ref native) = native_type
                    && !crate::docblock::should_override_type_typed(&php_type, native)
                {
                    // The override was rejected (scalar blocking).
                    return VarOverrideResult::None;
                }

                let var_name = bytes_to_str(dv.name).to_string();
                scope.set(&var_name, resolved);
                // Scan for preceding docblocks.
                apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
                return VarOverrideResult::NoVar;
            }
        } else if let Expression::Variable(Variable::Direct(dv)) = expr {
            let var_name = bytes_to_str(dv.name).to_string();
            scope.set(&var_name, resolved);
            apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
            return VarOverrideResult::NoVar;
        }
    }

    VarOverrideResult::None
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
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__rhs_check", 0, &scope_resolver);
    super::super::resolution::extract_native_type_from_rhs(rhs, &var_ctx)
}

/// Scan backwards through `before` (content before a docblock we already
/// processed) for additional standalone `/** @var Type $var */` blocks.
/// Each discovered block's `@var` tags are applied to `scope`.  Stops as
/// soon as the text no longer ends with `*/` (after trimming).
pub(crate) fn apply_preceding_var_docblocks(
    before: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let mut remaining = before.trim_end();
    // Keep scanning as long as the preceding text ends with a docblock.
    while remaining.ends_with("*/") {
        let doc_end = remaining.len();
        let doc_start = match remaining.rfind("/**") {
            Some(pos) => pos,
            None => break,
        };
        let doc_text = &remaining[doc_start..doc_end];
        let vars = parse_all_inline_var_docblocks(doc_text, ctx);
        if vars.is_empty() {
            // Not a @var docblock — stop scanning.
            break;
        }
        for (var_name, php_type) in &vars {
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
        }
        remaining = remaining[..doc_start].trim_end();
    }
}

/// Parse `/** @var Type $varName */` and return (var_name, PhpType).
/// Resolve a [`PhpType`] to a complete `Vec<ResolvedType>` with
/// `class_info` populated when possible.  Falls back to a
/// type-string-only entry for scalars and unresolvable types.
pub(crate) fn resolve_type_to_resolved_types(
    php_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        php_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    if !classes.is_empty() {
        ResolvedType::from_classes_with_hint(classes, php_type.clone())
    } else {
        vec![ResolvedType::from_type_string(php_type.clone())]
    }
}

/// Strip the `/**`…`*/` wrapper from a docblock and collapse its
/// line-continuation markers into a single space-joined string.
///
/// This flattens type strings that span multiple lines (e.g. a
/// `array{...}` shape written across several ` * ` lines) so they can be
/// parsed as one token sequence instead of retaining the leading `*`
/// markers, which [`PhpType::parse`] cannot interpret.
pub(crate) fn flatten_docblock_inner(doc_text: &str) -> Option<String> {
    let inner = doc_text.strip_prefix("/**")?.strip_suffix("*/")?;
    Some(
        inner
            .lines()
            .map(|l| l.trim().trim_start_matches('*').trim())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Parse ALL `@var Type $varName` pairs from a docblock.  Returns an
/// empty vec when none are found.  Handles multi-line docblocks with one
/// annotation per line as well as a single annotation whose type spans
/// several lines:
/// ```text
/// /**
///  * @var App                      $app
///  * @var array{indexName: string} $params
///  */
/// ```
/// ```text
/// /**
///  * @var array{
///  *     Label,
///  *     Stmt,
///  * } $pair
///  */
/// ```
pub(crate) fn parse_var_docblock_pairs(doc_text: &str) -> Vec<(String, PhpType)> {
    let inner = match flatten_docblock_inner(doc_text) {
        Some(s) => s,
        None => return vec![],
    };
    let inner = inner.as_str();

    let mut results = Vec::new();

    // Split on `@var` and process each occurrence.
    let mut search_from = 0;
    while let Some(pos) = inner[search_from..].find("@var") {
        let abs_pos = search_from + pos;
        let after = inner[abs_pos + 4..].trim_start();

        // Find the `$` that starts the variable name.  The type string
        // may contain spaces (e.g. `array<string, int>`).
        if let Some(dollar_pos) = after.find('$') {
            if dollar_pos > 0
                && let type_str = after[..dollar_pos].trim()
                && !type_str.is_empty()
                && let rest = &after[dollar_pos..]
                && let Some(var_name) = rest.split_whitespace().next()
                && !var_name.is_empty()
            {
                let php_type = PhpType::parse(type_str);
                results.push((var_name.to_string(), php_type));
            }
            search_from = abs_pos + 4 + dollar_pos + 1;
        } else {
            // No `$` after this @var — skip it.
            search_from = abs_pos + 4;
        }
    }

    results
}

/// Parse ALL `@var Type $varName` pairs from a docblock preceding an
/// assignment or expression.
pub(crate) fn parse_all_inline_var_docblocks(
    doc_text: &str,
    _ctx: &ForwardWalkCtx<'_>,
) -> Vec<(String, PhpType)> {
    parse_var_docblock_pairs(doc_text)
}

/// Parse ALL `@var Type $varName` annotations from a docblock.
/// Supports single-line (`/** @var Type $var */`), one-annotation-per-line
/// multi-line docblocks, and annotations whose type spans several lines
/// (e.g. a multi-line `array{...}` shape).
pub(crate) fn parse_all_var_docblock_annotations(doc_text: &str) -> Vec<(String, PhpType)> {
    parse_var_docblock_pairs(doc_text)
}

/// Parse `/** @var Type */` (without variable name) and return the PhpType.
pub(crate) fn parse_inline_var_docblock_no_var(
    doc_text: &str,
    _ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // Flatten line-continuation markers so a `array{...}` shape spread
    // across several lines is parsed as one type string.
    let inner = flatten_docblock_inner(doc_text)?;
    let inner = inner.trim().strip_prefix("@var")?.trim();

    // Stop at the next docblock tag so trailing tags (e.g. `@psalm-suppress`)
    // do not corrupt the type string.
    let type_str = match inner.find(" @") {
        Some(pos) => inner[..pos].trim(),
        None => inner,
    };
    // Strip a trailing `*` that may remain from `* @var Type *` formatting.
    let type_str = type_str.trim_end_matches('*').trim();

    // If there's a `$` it has a variable name — not the no-var form.
    if type_str.contains('$') {
        return None;
    }

    if type_str.is_empty() {
        return None;
    }

    Some(PhpType::parse(type_str))
}

/// Process assignment expressions, updating the scope.
pub(crate) fn process_assignment_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Expression::Assignment(assignment) = expr {
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

        // Array push: `$var[] = expr;`
        if let Expression::ArrayAppend(array_append) = assignment.lhs {
            if let Expression::Variable(Variable::Direct(dv)) = array_append.array {
                let var_name = bytes_to_str(dv.name).to_string();
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                if !rhs_types.is_empty() {
                    let value_type = ResolvedType::types_joined(&rhs_types);
                    let base_type = scope
                        .get(&var_name)
                        .last()
                        .map(|rt| rt.type_string.clone())
                        .unwrap_or_else(PhpType::array);
                    if !base_type.is_array_shape() {
                        let merged =
                            super::super::resolution::merge_push_type(&base_type, &value_type);
                        scope.set(&var_name, vec![ResolvedType::from_type_string(merged)]);
                    }
                }
            }
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
        if matches!(
            assignment.lhs,
            Expression::Access(Access::Property(_) | Access::NullSafeProperty(_))
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
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                if !rhs_types.is_empty() {
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

        let mut rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        // When the RHS is a numeric string literal (e.g. "123", '4.5'),
        // refine the type from `string` to `numeric-string` so that
        // downstream increment/decrement inference can detect it.
        if let Expression::Literal(Literal::String(lit_str)) = assignment.rhs {
            let raw = bytes_to_str(lit_str.raw).to_string();
            let unquoted = raw
                .strip_prefix('\'')
                .or_else(|| raw.strip_prefix('"'))
                .and_then(|s| s.strip_suffix('\'').or_else(|| s.strip_suffix('"')))
                .unwrap_or(&raw);
            if unquoted.parse::<i64>().is_ok() || unquoted.parse::<f64>().is_ok() {
                for rt in &mut rhs_types {
                    if rt.type_string.is_subtype_of(&PhpType::string()) {
                        rt.type_string = PhpType::Named("numeric-string".into());
                    }
                }
            }
        }
        // Reassigning the variable replaces its object identity, so any
        // property/array-access key rooted at it (seeded by an earlier
        // assignment or condition narrowing) is now stale.  Drop them
        // after resolving the RHS, so `$x = $x->foo` still reads the old
        // key while resolving.
        scope.invalidate_dependent_keys(&lhs_name);
        if !rhs_types.is_empty() {
            scope.set(&lhs_name, rhs_types);
        } else if !scope.contains(&lhs_name) {
            scope.set_empty(&lhs_name);
        }
    }
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
        _ => return,
    };
    // `??=` is handled separately: its result is the union of the LHS
    // (with `null` stripped) and the RHS.  We combine the resolved types
    // directly so the `class_info` already attached to each operand is
    // preserved — collapsing to a freshly built union *type string* would
    // discard it and force a re-resolution that fails for some subjects.
    if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) {
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        let mut combined: Vec<ResolvedType> = Vec::new();
        for lt in scope.get(&var_name) {
            // Drop a bare `null` member — `??=` only keeps the LHS when it
            // is non-null.
            if lt.type_string.is_null() {
                continue;
            }
            let mut kept = lt.clone();
            if let Some(non_null) = kept.type_string.non_null_type() {
                kept.type_string = non_null;
            }
            combined.push(kept);
        }
        combined.extend(rhs_types);
        // Deduplicate by type string so an identical LHS/RHS type (e.g.
        // `Foo|null ??= new Foo()`) does not produce a redundant union.
        let mut seen: Vec<PhpType> = Vec::new();
        combined.retain(|rt| {
            if seen.contains(&rt.type_string) {
                false
            } else {
                seen.push(rt.type_string.clone());
                true
            }
        });
        if !combined.is_empty() {
            scope.set(&var_name, combined);
        } else if !scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
        return;
    }

    let result_type = match &assignment.operator {
        AssignmentOperator::Concat(_) => PhpType::string(),
        AssignmentOperator::Modulo(_) => PhpType::int(),
        AssignmentOperator::LeftShift(_)
        | AssignmentOperator::RightShift(_)
        | AssignmentOperator::BitwiseAnd(_)
        | AssignmentOperator::BitwiseOr(_)
        | AssignmentOperator::BitwiseXor(_) => PhpType::int(),
        AssignmentOperator::Addition(_) => {
            // PHP overloads `+` / `+=` for array union vs numeric addition.
            // If either operand is array-like, the result is array.
            let lhs_types = scope.get(&var_name).to_vec();
            let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
            let either_is_array = lhs_types
                .iter()
                .chain(rhs_types.iter())
                .any(|rt| rt.type_string.is_array_like());
            if either_is_array {
                PhpType::Named("array".to_string())
            } else {
                infer_arithmetic_result_type(&lhs_types, &rhs_types, false)
            }
        }
        AssignmentOperator::Subtraction(_)
        | AssignmentOperator::Multiplication(_)
        | AssignmentOperator::Division(_)
        | AssignmentOperator::Exponentiation(_) => {
            let lhs_types = scope.get(&var_name).to_vec();
            let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
            let is_division = matches!(assignment.operator, AssignmentOperator::Division(_));
            infer_arithmetic_result_type(&lhs_types, &rhs_types, is_division)
        }
        AssignmentOperator::Coalesce(_) | AssignmentOperator::Assign(_) => return, // handled above / elsewhere
    };

    scope.set(&var_name, vec![ResolvedType::from_type_string(result_type)]);
}

/// Unwrap parenthesized expressions to their inner expression.
pub(crate) fn unwrap_parens<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::Parenthesized(p) => unwrap_parens(p.expression),
        other => other,
    }
}

/// Classify a resolved operand as `int`, `float`, or unknown for
/// arithmetic type promotion.
///
/// Returns `Some(true)` for float, `Some(false)` for int/bool,
/// `None` when the type is mixed or otherwise ambiguous.
/// Handles unions and nullable types by classifying each member.
pub(crate) fn classify_numeric_operand(types: &[ResolvedType]) -> Option<bool> {
    if types.is_empty() {
        return None;
    }
    let mut saw_float = false;
    let mut saw_int = false;
    for rt in types {
        classify_php_type(&rt.type_string, &mut saw_float, &mut saw_int)?;
    }
    if saw_float && saw_int {
        // Both int-like and float-like members present (e.g. int|float
        // union) — the runtime result could be either, so return None
        // to fall back to the conservative int|float.
        None
    } else if saw_float {
        Some(true)
    } else if saw_int {
        Some(false)
    } else {
        None
    }
}

/// Recursively classify a `PhpType` as int-like or float-like.
///
/// Returns `None` (and short-circuits) if any member is ambiguous
/// (mixed, string, object, etc.).  Updates `saw_float` and `saw_int`
/// flags for known numeric members.  `null` members are ignored
/// since they coerce to 0 in arithmetic context.
pub(crate) fn classify_php_type(
    ty: &PhpType,
    saw_float: &mut bool,
    saw_int: &mut bool,
) -> Option<()> {
    match ty {
        PhpType::Named(n) => {
            let lower = n.to_ascii_lowercase();
            if lower == "float" || lower == "double" || lower == "real" {
                *saw_float = true;
            } else if lower == "int"
                || lower == "integer"
                || lower == "bool"
                || lower == "boolean"
                || lower == "true"
                || lower == "false"
            {
                *saw_int = true;
            } else if lower == "numeric" || lower == "number" {
                *saw_int = true;
                *saw_float = true;
            } else if lower == "null" {
                // null coerces to 0 (int) in arithmetic; ignore it
                // so that `int|null` classifies as int-like.
            } else {
                return None; // mixed, string, object, etc.
            }
            Some(())
        }
        PhpType::Union(members) => {
            for member in members {
                classify_php_type(member, saw_float, saw_int)?;
            }
            Some(())
        }
        PhpType::Nullable(inner) => {
            // ?T is T|null — classify the inner type, ignore null.
            classify_php_type(inner, saw_float, saw_int)
        }
        _ => None,
    }
}

/// Infer the result type of an arithmetic operation based on operand
/// types, following PHP's numeric type promotion rules.
///
/// - `int op int` → `int` (for `+`, `-`, `*`, `**`)
/// - `int op float` or `float op int` → `float`
/// - `float op float` → `float`
/// - `int / int` → `int|float` (division can produce either)
/// - Anything else → `int|float`
pub(crate) fn infer_arithmetic_result_type(
    lhs_types: &[ResolvedType],
    rhs_types: &[ResolvedType],
    is_division: bool,
) -> PhpType {
    let lhs = classify_numeric_operand(lhs_types);
    let rhs = classify_numeric_operand(rhs_types);
    match (lhs, rhs) {
        // Both are known int (not float): int op int.
        (Some(false), Some(false)) => {
            if is_division {
                // int / int can return float (e.g. 7/2 = 3.5).
                PhpType::Union(vec![PhpType::int(), PhpType::float()])
            } else {
                PhpType::int()
            }
        }
        // At least one float, the other is known: result is float.
        (Some(true), Some(_)) | (Some(_), Some(true)) => PhpType::float(),
        // One or both operands are unknown: fall back to int|float.
        _ => PhpType::Union(vec![PhpType::int(), PhpType::float()]),
    }
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
        let result_type = match &assignment.operator {
            AssignmentOperator::Concat(_) => Some(PhpType::string()),
            AssignmentOperator::Modulo(_) => Some(PhpType::int()),
            AssignmentOperator::LeftShift(_)
            | AssignmentOperator::RightShift(_)
            | AssignmentOperator::BitwiseAnd(_)
            | AssignmentOperator::BitwiseOr(_)
            | AssignmentOperator::BitwiseXor(_) => Some(PhpType::int()),
            AssignmentOperator::Addition(_) => {
                // PHP overloads `+` / `+=` for array union vs numeric addition.
                let lhs_types = if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    scope.get(bytes_to_str(dv.name)).to_vec()
                } else {
                    vec![]
                };
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                let either_is_array = lhs_types
                    .iter()
                    .chain(rhs_types.iter())
                    .any(|rt| rt.type_string.is_array_like());
                if either_is_array {
                    Some(PhpType::Named("array".to_string()))
                } else {
                    Some(infer_arithmetic_result_type(&lhs_types, &rhs_types, false))
                }
            }
            AssignmentOperator::Subtraction(_)
            | AssignmentOperator::Multiplication(_)
            | AssignmentOperator::Division(_)
            | AssignmentOperator::Exponentiation(_) => {
                let lhs_types = if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    scope.get(bytes_to_str(dv.name)).to_vec()
                } else {
                    vec![]
                };
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                let is_division = matches!(assignment.operator, AssignmentOperator::Division(_));
                Some(infer_arithmetic_result_type(
                    &lhs_types,
                    &rhs_types,
                    is_division,
                ))
            }
            AssignmentOperator::Coalesce(_) => {
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                let rhs_type = if rhs_types.is_empty() {
                    PhpType::mixed()
                } else {
                    ResolvedType::types_joined(&rhs_types)
                };
                Some(rhs_type)
            }
            AssignmentOperator::Assign(_) => None,
        };
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
            let class_string_type =
                PhpType::ClassString(Some(Box::new(PhpType::Named(resolved_name.to_string()))));
            let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
                &PhpType::Named(resolved_name.to_string()),
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
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

    // Type casts: (int)$x → int, (string)$x → string, etc.
    if let Expression::UnaryPrefix(prefix) = rhs {
        use mago_syntax::cst::unary::UnaryPrefixOperator;
        let cast_type = match &prefix.operator {
            UnaryPrefixOperator::IntCast(..) | UnaryPrefixOperator::IntegerCast(..) => {
                Some(PhpType::int())
            }
            UnaryPrefixOperator::StringCast(..) | UnaryPrefixOperator::BinaryCast(..) => {
                Some(PhpType::string())
            }
            UnaryPrefixOperator::FloatCast(..)
            | UnaryPrefixOperator::DoubleCast(..)
            | UnaryPrefixOperator::RealCast(..) => Some(PhpType::float()),
            UnaryPrefixOperator::BoolCast(..) | UnaryPrefixOperator::BooleanCast(..) => {
                Some(PhpType::bool())
            }
            UnaryPrefixOperator::ArrayCast(..) => Some(PhpType::array()),
            UnaryPrefixOperator::ObjectCast(..) => {
                // Resolve the operand type to produce an object shape:
                // - scalar → object{scalar: <type>}
                // - array shape → object{key: type, ...}
                // - otherwise → stdClass
                let operand_types = resolve_rhs_with_scope(prefix.operand, scope, ctx);
                let inner = operand_types.first().map(|rt| &rt.type_string).cloned();
                let obj_type = match inner {
                    Some(PhpType::ArrayShape(entries)) => {
                        // Widen literal types to their base types:
                        // PHP (object) cast doesn't preserve literal precision.
                        let widened = entries
                            .into_iter()
                            .map(|mut e| {
                                e.value_type = widen_literal(&e.value_type);
                                e
                            })
                            .collect();
                        PhpType::ObjectShape(widened)
                    }
                    Some(ref ty) if matches!(ty, PhpType::Named(s) if matches!(s.to_ascii_lowercase().as_str(), "int" | "integer" | "string" | "float" | "double" | "real" | "bool" | "boolean")) => {
                        PhpType::ObjectShape(vec![ShapeEntry {
                            key: Some("scalar".to_string()),
                            value_type: ty.clone(),
                            optional: false,
                        }])
                    }
                    _ => PhpType::Named("stdClass".into()),
                };
                Some(obj_type)
            }
            UnaryPrefixOperator::UnsetCast(..) => Some(PhpType::Named("null".into())),
            UnaryPrefixOperator::Negation(_) | UnaryPrefixOperator::Plus(_) => {
                // Unary +/- preserves int or float; conservatively
                // return int|float.
                Some(PhpType::Union(vec![PhpType::int(), PhpType::float()]))
            }
            UnaryPrefixOperator::BitwiseNot(_) => None, // handled below
            UnaryPrefixOperator::Not(_) => Some(PhpType::bool()),
            _ => None,
        };
        if let Some(ty) = cast_type {
            return vec![ResolvedType::from_type_string(ty)];
        }
    }

    // Bitwise NOT (~): returns string when operand is string, int otherwise.
    if let Expression::UnaryPrefix(prefix) = rhs {
        use mago_syntax::cst::unary::UnaryPrefixOperator;
        if matches!(prefix.operator, UnaryPrefixOperator::BitwiseNot(_)) {
            let operand_types = resolve_rhs_with_scope(prefix.operand, scope, ctx);
            let is_string = !operand_types.is_empty()
                && operand_types
                    .iter()
                    .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()));
            return vec![ResolvedType::from_type_string(if is_string {
                PhpType::string()
            } else {
                PhpType::int()
            })];
        }
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
    let var_ctx = ctx.var_ctx_for_with_scope(dummy_var, rhs_offset, &scope_resolver);

    let result = super::super::rhs_resolution::resolve_rhs_expression(rhs, &var_ctx);
    if !result.is_empty() {
        return result;
    }

    // ── Structural fallbacks ────────────────────────────────────
    // When resolve_rhs_expression returns empty, infer the type
    // purely from the expression structure.  These only fire as a
    // last resort so they never override a more precise result.

    // Unwrap parenthesized expressions for structural inference.
    let rhs = unwrap_parens(rhs);

    // String literals (including interpolated/composite strings).
    if matches!(
        rhs,
        Expression::Literal(Literal::String(_)) | Expression::CompositeString(_)
    ) {
        return vec![ResolvedType::from_type_string(PhpType::string())];
    }

    // Integer literals.
    if matches!(rhs, Expression::Literal(Literal::Integer(_))) {
        return vec![ResolvedType::from_type_string(PhpType::int())];
    }

    // Float literals.
    if matches!(rhs, Expression::Literal(Literal::Float(_))) {
        return vec![ResolvedType::from_type_string(PhpType::float())];
    }

    // Boolean and null literals.
    if matches!(
        rhs,
        Expression::Literal(Literal::True(_) | Literal::False(_))
    ) {
        return vec![ResolvedType::from_type_string(PhpType::bool())];
    }
    if matches!(rhs, Expression::Literal(Literal::Null(_))) {
        return vec![ResolvedType::from_type_string(PhpType::Named(
            "null".into(),
        ))];
    }

    // Binary operators — the result type depends on the operator kind.
    if let Expression::Binary(binary) = rhs {
        use mago_syntax::cst::binary::BinaryOperator;

        // Spaceship (<=>): always int (-1, 0, or 1).
        if matches!(binary.operator, BinaryOperator::Spaceship(_)) {
            return vec![ResolvedType::from_type_string(PhpType::int())];
        }

        // instanceof, comparison, logical: always bool.
        if binary.operator.is_instanceof()
            || binary.operator.is_comparison()
            || binary.operator.is_logical()
        {
            return vec![ResolvedType::from_type_string(PhpType::bool())];
        }

        // Concatenation (.): always string.
        if matches!(binary.operator, BinaryOperator::StringConcat(_)) {
            return vec![ResolvedType::from_type_string(PhpType::string())];
        }

        // Modulo (%): always int.
        if matches!(binary.operator, BinaryOperator::Modulo(_)) {
            return vec![ResolvedType::from_type_string(PhpType::int())];
        }

        // Addition (+): PHP overloads this for array union vs numeric
        // addition.  If either operand resolves to an array type, the
        // result is array; otherwise apply numeric type promotion.
        if matches!(binary.operator, BinaryOperator::Addition(_)) {
            let lhs_types = resolve_rhs_with_scope(binary.lhs, scope, ctx);
            let rhs_types = resolve_rhs_with_scope(binary.rhs, scope, ctx);
            let either_is_array = lhs_types
                .iter()
                .chain(rhs_types.iter())
                .any(|rt| rt.type_string.is_array_like());
            if either_is_array {
                return vec![ResolvedType::from_type_string(PhpType::Named(
                    "array".to_string(),
                ))];
            }
            return vec![ResolvedType::from_type_string(
                infer_arithmetic_result_type(&lhs_types, &rhs_types, false),
            )];
        }

        // Arithmetic: -, *, /, **.
        if matches!(
            binary.operator,
            BinaryOperator::Subtraction(_)
                | BinaryOperator::Multiplication(_)
                | BinaryOperator::Division(_)
                | BinaryOperator::Exponentiation(_)
        ) {
            let lhs_types = resolve_rhs_with_scope(binary.lhs, scope, ctx);
            let rhs_types = resolve_rhs_with_scope(binary.rhs, scope, ctx);
            let is_division = matches!(binary.operator, BinaryOperator::Division(_));
            return vec![ResolvedType::from_type_string(
                infer_arithmetic_result_type(&lhs_types, &rhs_types, is_division),
            )];
        }

        // Bitwise operators (&, |, ^, <<, >>).
        // When both operands are strings, PHP applies bitwise ops
        // character-by-character and returns a string.  Otherwise int.
        if matches!(
            binary.operator,
            BinaryOperator::BitwiseAnd(_)
                | BinaryOperator::BitwiseOr(_)
                | BinaryOperator::BitwiseXor(_)
                | BinaryOperator::LeftShift(_)
                | BinaryOperator::RightShift(_)
        ) {
            // Check if both operands are string-typed for &, |, ^.
            if matches!(
                binary.operator,
                BinaryOperator::BitwiseAnd(_)
                    | BinaryOperator::BitwiseOr(_)
                    | BinaryOperator::BitwiseXor(_)
            ) {
                let lhs_types = resolve_rhs_with_scope(binary.lhs, scope, ctx);
                let rhs_types = resolve_rhs_with_scope(binary.rhs, scope, ctx);
                let both_strings = !lhs_types.is_empty()
                    && !rhs_types.is_empty()
                    && lhs_types
                        .iter()
                        .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()))
                    && rhs_types
                        .iter()
                        .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()));
                if both_strings {
                    return vec![ResolvedType::from_type_string(PhpType::string())];
                }
            }
            return vec![ResolvedType::from_type_string(PhpType::int())];
        }
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
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__rhs_subject", 0, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();

    // Determine the access kind from the expression text.
    let access_kind = if rhs_text.contains("::") {
        crate::types::AccessKind::DoubleColon
    } else {
        crate::types::AccessKind::Arrow
    };

    crate::completion::resolver::resolve_target_classes(rhs_text, access_kind, &rctx)
}

/// Process array destructuring assignments.
///
/// Resolves the RHS type once, then walks the LHS pattern to assign
/// types to each destructured variable.  Handles nested patterns like
/// `[$a, [$b, $c]] = $nested` by recursing into inner array/list
/// expressions.
pub(crate) fn process_destructuring_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };

    // Build a temporary VarResolutionCtx just to resolve the RHS type.
    // The var_name doesn't matter here since we're resolving the RHS
    // expression, not looking up a specific variable.
    let dummy_name = String::from("$__destructuring_rhs");
    let var_ctx = VarResolutionCtx {
        var_name: &dummy_name,
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: assignment.span().start.offset,
        class_loader: ctx.class_loader,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        branch_aware: false,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: Some(&scope_resolver),
    };

    // Try inline @var docblock first, then fall back to RHS expression.
    let stmt_offset = assignment.span().start.offset as usize;
    let raw_type: Option<PhpType> =
        crate::docblock::find_inline_var_docblock(ctx.content, stmt_offset)
            .map(|(vt, _)| crate::util::resolve_php_type_names(&vt, ctx.class_loader))
            .or_else(|| {
                super::super::foreach_resolution::resolve_expression_type(assignment.rhs, &var_ctx)
            });

    // Expand type aliases before shape/generic extraction.
    let raw_type = raw_type.map(|rt| {
        crate::completion::type_resolution::resolve_type_alias_typed(
            &rt,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(rt)
    });

    if let Some(ref rhs_type) = raw_type {
        bind_destructured_pattern(assignment.lhs, rhs_type, scope, ctx);
    }

    // Ensure every destructured variable is present in scope even when the
    // RHS type (or an individual element's type) could not be resolved.  A
    // plain assignment from an unresolvable RHS records the variable with an
    // empty type list via `set_empty`, which lets later assert narrowing seed
    // a type for it.  Without this, list-destructuring from an unresolvable
    // RHS leaves the variables absent from scope entirely, so the assert
    // narrowing loop never visits them and the asserted type is dropped.
    seed_destructured_vars_empty(assignment.lhs, scope);
}

/// Walk a destructuring LHS pattern and record every direct variable in
/// scope with an empty type list, unless it is already present.  Used so
/// that variables destructured from an unresolvable RHS still participate
/// in later narrowing (`set_empty` leaves any already-bound type intact).
pub(crate) fn seed_destructured_vars_empty<'b>(lhs: &'b Expression<'b>, scope: &mut ScopeState) {
    let elements: Vec<&ArrayElement<'b>> = match lhs {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return,
    };

    for elem in elements {
        let value_expr = match elem {
            ArrayElement::KeyValue(kv) => kv.value,
            ArrayElement::Value(val) => val.value,
            _ => continue,
        };
        match value_expr {
            Expression::Variable(Variable::Direct(dv)) => {
                scope.set_empty(bytes_to_str(dv.name));
            }
            Expression::Array(_) | Expression::List(_) => {
                seed_destructured_vars_empty(value_expr, scope);
            }
            _ => {}
        }
    }
}

/// Recursively bind types from a destructuring LHS pattern against a
/// resolved RHS type.  For each variable in the pattern, extracts the
/// corresponding type from the RHS type (via shape key or positional
/// index) and sets it in scope.  For nested array/list sub-patterns,
/// recurses with the extracted element type.
pub(crate) fn bind_destructured_pattern<'b>(
    lhs: &'b Expression<'b>,
    rhs_type: &PhpType,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let elements: Vec<&ArrayElement<'b>> = match lhs {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return,
    };

    let mut positional_index: usize = 0;
    for elem in elements {
        let (value_expr, shape_key) = match elem {
            ArrayElement::KeyValue(kv) => {
                let key = extract_foreach_destr_key(kv.key);
                (kv.value, key)
            }
            ArrayElement::Value(val) => {
                let key = Some(positional_index.to_string());
                positional_index += 1;
                (val.value, key)
            }
            _ => continue,
        };

        // Determine the type for this element position.
        let elem_type: Option<PhpType> = shape_key
            .as_ref()
            .and_then(|k| rhs_type.shape_value_type(k).cloned())
            .or_else(|| rhs_type.extract_value_type(false).cloned());

        match value_expr {
            // Direct variable: bind the type.
            Expression::Variable(Variable::Direct(dv)) => {
                if let Some(ref vt) = elem_type {
                    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                        vt,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                    let resolved_types = if !resolved.is_empty() {
                        ResolvedType::from_classes_with_hint(resolved, vt.clone())
                    } else {
                        vec![ResolvedType::from_type_string(vt.clone())]
                    };
                    scope.set(bytes_to_str(dv.name), resolved_types);
                }
            }
            // Nested pattern: recurse with the extracted element type.
            Expression::Array(_) | Expression::List(_) => {
                if let Some(ref vt) = elem_type {
                    bind_destructured_pattern(value_expr, vt, scope, ctx);
                }
            }
            _ => {}
        }
    }
}

/// Process array key assignment: `$var['key'] = expr;`
pub(crate) fn process_array_key_assignment<'b>(
    _array_access: &'b ArrayAccess<'b>,
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Delegate to the existing check_expression_for_assignment
    // infrastructure for array key assignments.  This handles
    // both string-keyed shape building and generic element tracking.
    //
    // We iterate over all variables currently in scope and check
    // whether the assignment targets any of them.
    // For simplicity in Phase 1, use the existing path.
    // Extract the base variable name from the array access.
    if let Some((base_name, key_chain)) =
        super::super::resolution::extract_nested_array_access_chain(_array_access)
    {
        // Resolve the RHS value type.
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        let value_php_type = if !rhs_types.is_empty() {
            ResolvedType::types_joined(&rhs_types)
        } else {
            PhpType::mixed()
        };
        let base_type = scope
            .get(&base_name)
            .last()
            .map(|rt| rt.type_string.clone())
            .unwrap_or_else(PhpType::array);

        // If the base variable is an object (e.g. SplObjectStorage, ArrayAccess),
        // array-access syntax invokes offsetSet, not actual array mutation.
        // Preserve the original object type instead of overwriting it with an array shape.
        if base_type.is_object_like() && !base_type.is_array_like() {
            return;
        }

        // If the base variable is a string, bracket-indexed assignment
        // (`$str[0] = 'z'`) modifies the string in-place — the variable
        // remains a string, it does NOT become an array.
        if base_type.is_string_subtype() {
            return;
        }

        // Extract all keys in the chain.
        let all_string_keys: Option<Vec<String>> = key_chain
            .iter()
            .map(|idx| super::super::resolution::extract_array_key_for_shape(idx))
            .collect();

        if let Some(keys) = all_string_keys {
            let merged = super::super::resolution::merge_nested_shape_keys(
                &base_type,
                &keys,
                &value_php_type,
            );
            scope.set(&base_name, vec![ResolvedType::from_type_string(merged)]);
        } else {
            // The chain contains at least one dynamic (non-literal) key,
            // e.g. `$sums[$id] = …` or `$return['data'][$count]['earnings']
            // = …`.  Literal segments are tracked as shape entries and
            // dynamic segments as generic `array<K, V>` levels.
            let rhs_offset = assignment.span().start.offset;
            let scope_locals = &scope.locals;
            let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
                scope_locals
                    .get(&atom(var_name))
                    .cloned()
                    .unwrap_or_default()
            };
            let rhs_ctx = ctx.var_ctx_for_with_scope("$__idx", rhs_offset, &scope_resolver);
            let write_keys: Vec<super::super::resolution::ArrayWriteKey> = key_chain
                .iter()
                .map(
                    |idx| match super::super::resolution::extract_array_key_for_shape(idx) {
                        Some(key) => super::super::resolution::ArrayWriteKey::Shape(key),
                        None => super::super::resolution::ArrayWriteKey::Keyed(
                            super::super::resolution::infer_array_key_type(idx, &rhs_ctx),
                        ),
                    },
                )
                .collect();
            let merged = super::super::resolution::merge_nested_array_write(
                &base_type,
                &write_keys,
                &value_php_type,
            );
            scope.set(&base_name, vec![ResolvedType::from_type_string(merged)]);
        }
    }
}

/// Process pass-by-reference parameter type inference.
pub(crate) fn process_pass_by_ref<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When a function call passes a variable to a parameter declared
    // as `Type &$param`, the variable acquires that type after the call.
    //
    // We need to check both variables already in scope AND variables
    // that appear as arguments but don't exist in scope yet (e.g.
    // `$matches` in `preg_match($pattern, $subject, $matches)`).
    //
    // Phase 1: use the existing `try_apply_pass_by_reference_type`
    // infrastructure for variables already in scope (works for class
    // types like `Type &$param`).
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };

    // Collect all variable names that appear as arguments in this
    // expression, including ones not yet in scope.
    let mut all_var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    for arg_var in extract_call_arg_variables(expr) {
        if !all_var_names.contains(&arg_var) {
            all_var_names.push(arg_var);
        }
    }

    for var_name in all_var_names {
        let var_ctx = VarResolutionCtx {
            var_name: &var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(&scope_resolver),
        };
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();
        super::super::resolution::try_apply_pass_by_reference_type(
            expr,
            &var_ctx,
            &mut results,
            false,
        );
        if results.len() != before.len() {
            scope.set(&var_name, results);
        }
    }

    // Phase 2: for variables NOT yet in scope that are passed to
    // pass-by-reference parameters with primitive type hints (e.g.
    // `array &$matches` in `preg_match`), store the type hint
    // directly.  `try_apply_pass_by_reference_type` only produces
    // results for class-based type hints; primitive types like
    // `array`, `int`, `string` return empty from
    // `type_hint_to_classes_typed` and are missed.
    seed_pass_by_ref_primitives(expr, scope, ctx);
}

/// Seed PHP superglobals (`$_SERVER`, `$_GET`, `$_POST`, etc.) into the
/// scope as `array` so that accesses on them resolve correctly.
/// PHP makes these available in every scope without
/// an explicit `global` declaration.
pub(crate) fn seed_superglobals(scope: &mut ScopeState) {
    let array_type = vec![ResolvedType::from_type_string(PhpType::Named(
        "array".to_string(),
    ))];
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

/// Recursively walk an expression tree to find function call
/// sub-expressions and seed pass-by-reference primitive types for each.
/// This handles patterns like `if (preg_match($pattern, $subject, $matches))`
/// and `if (preg_match(..., $matches) === 1)` where the call is nested
/// inside a comparison or logical expression rather than appearing as a
/// standalone expression statement.
///
/// Only uses [`seed_pass_by_ref_primitives`] (not the full
/// [`process_pass_by_ref`]) to avoid triggering recursive variable
/// resolution through `try_apply_pass_by_reference_type`, which would
/// inflate the fallthrough counter for every variable already in scope.
pub(crate) fn seed_pass_by_ref_in_condition<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        // Direct call expressions — seed primitive pass-by-ref types.
        Expression::Call(_) => {
            seed_pass_by_ref_primitives(expr, scope, ctx);
        }
        // Binary operators (e.g. `preg_match(...) === 1`, `a && b`)
        // — recurse into both sides.
        Expression::Binary(bin) => {
            seed_pass_by_ref_in_condition(bin.lhs, scope, ctx);
            seed_pass_by_ref_in_condition(bin.rhs, scope, ctx);
        }
        // Unary prefix (e.g. `!preg_match(...)`) — recurse into operand.
        Expression::UnaryPrefix(unary) => {
            seed_pass_by_ref_in_condition(unary.operand, scope, ctx);
        }
        // Unary postfix — recurse into operand.
        Expression::UnaryPostfix(unary) => {
            seed_pass_by_ref_in_condition(unary.operand, scope, ctx);
        }
        // Parenthesized — recurse into inner expression.
        Expression::Parenthesized(paren) => {
            seed_pass_by_ref_in_condition(paren.expression, scope, ctx);
        }
        // Assignment in condition (e.g. `if ($x = preg_match(..., $m))`)
        // — recurse into the RHS.
        Expression::Assignment(assignment) => {
            seed_pass_by_ref_in_condition(assignment.rhs, scope, ctx);
        }
        _ => {}
    }
}

/// For each variable argument in a call expression that is passed to a
/// pass-by-reference parameter with a primitive type hint (e.g.
/// `array &$matches`), seed the variable in scope if it isn't already
/// there.  This complements [`process_pass_by_ref`] which handles
/// class-typed parameters via `try_apply_pass_by_reference_type`.
pub(crate) fn seed_pass_by_ref_primitives<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Resolve the called function/method's parameters.
    let (arg_list, parameters) = match expr {
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let func_name_offset = func_call.function.span().start.offset;
            let fl = match ctx.loaders.function_loader {
                Some(fl) => fl,
                None => return,
            };
            let func_info = match fl(&func_name, func_name_offset) {
                Some(fi) => fi,
                None => return,
            };
            (&func_call.argument_list, func_info.parameters)
        }
        Expression::Call(Call::Method(mc)) => {
            let method_name = match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let receiver_class = match mc.object {
                Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                    Some(ctx.current_class.name.to_string())
                }
                Expression::Variable(Variable::Direct(dv)) => {
                    let types = scope.get(bytes_to_str(dv.name));
                    types.iter().find_map(|rt| {
                        let name = rt.type_string.base_name()?;
                        if crate::php_type::is_primitive_scalar_name(name) {
                            None
                        } else {
                            Some(name.to_string())
                        }
                    })
                }
                _ => return,
            };
            let class_name = match receiver_class {
                Some(n) => n,
                None => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (&mc.argument_list, method.parameters.clone())
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            let method_name = match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let receiver_class = match mc.object {
                Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                    Some(ctx.current_class.name.to_string())
                }
                Expression::Variable(Variable::Direct(dv)) => {
                    let types = scope.get(bytes_to_str(dv.name));
                    types.iter().find_map(|rt| {
                        let name = rt.type_string.base_name()?;
                        if crate::php_type::is_primitive_scalar_name(name) {
                            None
                        } else {
                            Some(name.to_string())
                        }
                    })
                }
                _ => return,
            };
            let class_name = match receiver_class {
                Some(n) => n,
                None => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (&mc.argument_list, method.parameters.clone())
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let method_name = match &sc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let class_name = match sc.class {
                Expression::Self_(_) | Expression::Static(_) => ctx.current_class.name.to_string(),
                Expression::Parent(_) => match ctx.current_class.parent_class {
                    Some(p) => p.to_string(),
                    None => return,
                },
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (&sc.argument_list, method.parameters.clone())
        }
        _ => return,
    };

    // Bind arguments to parameters following PHP's rules so a named argument
    // seeds the parameter it actually targets, not the one at its ordinal
    // position in the call.
    let bound = crate::call_args::bind_args_to_params(&parameters, arg_list);

    for (param, arg_expr) in parameters.iter().zip(bound.iter()) {
        let arg_expr = match arg_expr {
            Some(expr) => *expr,
            None => continue,
        };

        // Only handle direct variable arguments.
        let var_name = match arg_expr {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => continue,
        };

        // Skip if already in scope (Phase 1 handled it).
        if !scope.get(&var_name).is_empty() {
            continue;
        }

        // Check if the corresponding parameter is pass-by-reference.
        if param.is_reference {
            if let Some(type_hint) = &param.type_hint {
                scope.set(
                    &var_name,
                    vec![ResolvedType::from_type_string(type_hint.clone())],
                );
            } else {
                // Untyped pass-by-reference parameters (e.g. `&$matches`
                // in `preg_match`, `&$result` in `parse_str`) are most
                // commonly arrays.  Seed as `array` so that subsequent
                // array accesses like `$matches[1]` don't fall through
                // to the backward scanner.
                scope.set(
                    &var_name,
                    vec![ResolvedType::from_type_string(PhpType::Named(
                        "array".to_string(),
                    ))],
                );
            }
        }
    }
}

/// Extract all `$variable` names that appear as direct arguments in a
/// call expression.  Used by [`process_pass_by_ref`] to discover
/// variables that may be introduced by pass-by-reference parameters
/// (e.g. `$matches` in `preg_match($pattern, $subject, $matches)`).
pub(crate) fn extract_call_arg_variables<'b>(expr: &'b Expression<'b>) -> Vec<String> {
    let arg_list = match expr {
        Expression::Call(Call::Function(fc)) => &fc.argument_list,
        Expression::Call(Call::Method(mc)) => &mc.argument_list,
        Expression::Call(Call::NullSafeMethod(mc)) => &mc.argument_list,
        Expression::Call(Call::StaticMethod(sc)) => &sc.argument_list,
        Expression::Instantiation(inst) => match &inst.argument_list {
            Some(al) => al,
            None => return vec![],
        },
        _ => return vec![],
    };
    let mut vars = Vec::new();
    for arg in arg_list.arguments.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
            vars.push(bytes_to_str(dv.name).to_string());
        }
    }
    vars
}

/// Process assert narrowing (assert($x instanceof Foo), @phpstan-assert, etc.)
pub(crate) fn process_assert_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // ── Handle assert($x instanceof Foo) for variables NOT yet in scope ──
    // When a foreach binds a variable but the iterable element type is
    // unknown, the variable won't be in the scope map.  A subsequent
    // `assert($x instanceof Foo)` should add it with the asserted type.
    if let Expression::Call(Call::Function(fc)) = expr
        && matches!(fc.function, Expression::Identifier(ident) if ident.value() == b"assert")
        && let Some(arg) = fc.argument_list.arguments.first()
    {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Expression::Binary(bin) = arg_expr
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
                    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                        &PhpType::Named(name.clone()),
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                    if !resolved.is_empty() {
                        scope.set(
                            &var_name,
                            ResolvedType::from_classes_with_hint(resolved, PhpType::Named(name)),
                        );
                    } else {
                        scope.set(
                            &var_name,
                            vec![ResolvedType::from_type_string(PhpType::Named(name))],
                        );
                    }
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

    // Apply assert narrowing to each variable in scope.
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
    for var_name in var_names {
        let var_ctx = VarResolutionCtx {
            var_name: &var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(&scope_resolver),
        };
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();

        // assert($x instanceof Foo)
        ResolvedType::apply_narrowing(&mut results, |classes| {
            narrowing::try_apply_assert_instanceof_narrowing(expr, &var_ctx, classes)
        });

        // @phpstan-assert / @psalm-assert
        let mut type_guard: Option<(narrowing::TypeGuardKind, bool)> = None;
        ResolvedType::apply_narrowing(&mut results, |classes| {
            narrowing::try_apply_custom_assert_narrowing(expr, &var_ctx, classes, &mut type_guard)
        });

        // A scalar / pseudo-type assertion (`assertIsString`, `assertIsObject`,
        // `assertIsArray`, their `assertIsNot*` negations, or the `object`
        // fallback for an unresolvable `assertInstanceOf` class argument) is a
        // type guard, not a class narrowing.  Apply it on the full resolved
        // types so union members are kept or dropped by category — e.g.
        // `assertIsObject` drops null/scalar members while keeping the class,
        // and `assertIsNotObject` drops the class.
        if let Some((kind, exclude)) = type_guard {
            if exclude {
                narrowing::apply_type_guard_exclusion(kind, &mut results);
            } else {
                narrowing::apply_type_guard_inclusion(kind, &mut results);
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
                scope.locals.insert(var_name, vec![]);
            } else {
                scope.set(&var_name, results);
            }
        }
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
