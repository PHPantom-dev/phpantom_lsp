use super::*;
use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{atom, bytes_to_str};
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::types::ResolvedType;

// ─── Control flow handling ──────────────────────────────────────────────────

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
    record_and_chain_snapshots(if_stmt.condition, scope, ctx);
    record_or_chain_snapshots(if_stmt.condition, scope, ctx);

    // Check if the cursor is inside the condition expression.
    // If so, apply inline && narrowing.
    let cond_span = if_stmt.condition.span();
    if ctx.cursor_offset >= cond_span.start.offset && ctx.cursor_offset <= cond_span.end.offset {
        // Cursor is in the condition — scope is already correct.
        return;
    }

    // Assignment in condition: `if ($x = expr())`
    process_condition_assignment(if_stmt.condition, scope, ctx);

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

    // Check if cursor is in any elseif body.
    let cursor_in_elseif = body.else_if_clauses.iter().any(|ei| {
        let sp = ei.statement.span();
        ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset
    });

    // Check if cursor is in else body.
    let cursor_in_else = body.else_clause.as_ref().is_some_and(|ec| {
        let sp = ec.statement.span();
        ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset
    });

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
                apply_condition_narrowing(ei.condition, scope, ctx);
                process_condition_assignment(ei.condition, scope, ctx);
                seed_pass_by_ref_in_condition(ei.condition, scope, ctx);
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

    // Walk each branch independently and merge results.
    let mut then_scope = scope.clone();
    apply_condition_narrowing(if_stmt.condition, &mut then_scope, ctx);
    walk_body_forward(std::iter::once(body.statement), &mut then_scope, ctx);
    let then_exits = statement_unconditionally_exits(body.statement);

    let mut elseif_scopes: Vec<(ScopeState, bool)> = Vec::new();
    for ei in body.else_if_clauses.iter() {
        let mut ei_scope = pre_if_scope.clone();
        apply_condition_narrowing_inverse(if_stmt.condition, &mut ei_scope, ctx);
        for (prev_idx, prev_ei) in body.else_if_clauses.iter().enumerate() {
            if std::ptr::eq(prev_ei, ei) {
                break;
            }
            apply_condition_narrowing_inverse(prev_ei.condition, &mut ei_scope, ctx);
            let _ = prev_idx;
        }
        // Record a scope snapshot at the elseif condition boundary so
        // that diagnostic variable lookups inside the condition don't
        // pick up assignments from preceding if/elseif bodies.
        if is_diagnostic_scope_active() {
            record_scope_snapshot(ei.condition.span().start.offset, &ei_scope);
        }
        apply_condition_narrowing(ei.condition, &mut ei_scope, ctx);
        process_condition_assignment(ei.condition, &mut ei_scope, ctx);
        seed_pass_by_ref_in_condition(ei.condition, &mut ei_scope, ctx);
        walk_body_forward(std::iter::once(ei.statement), &mut ei_scope, ctx);
        let exits = statement_unconditionally_exits(ei.statement);
        elseif_scopes.push((ei_scope, exits));
    }

    let (else_scope, else_exits) = if let Some(ref else_clause) = body.else_clause {
        let mut else_scope = pre_if_scope.clone();
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
        let exits = statement_unconditionally_exits(else_clause.statement);
        (Some(else_scope), exits)
    } else {
        (None, false)
    };

    // Merge: collect all surviving (non-exiting) branch scopes.
    // Branches that exit via break/continue are loop-local exits —
    // their variable assignments flow to the post-loop scope, so
    // they must be included in the merge alongside truly surviving
    // branches.
    //
    // When there is no else clause, the pre-if scope represents the
    // implicit "condition was false" path.  We apply inverse condition
    // narrowing to it so that information from the condition (e.g.
    // `$a["test"] === null` → `$a["test"]` is NOT null in the else
    // path) is reflected in the merge.
    let mut implicit_else_scope;
    let mut surviving_scopes: Vec<&ScopeState> = Vec::new();

    let then_exits_via_loop = exits_via_loop_control(body.statement);
    if !then_exits || then_exits_via_loop {
        surviving_scopes.push(&then_scope);
    }
    for (idx, (ei_scope, ei_exits)) in elseif_scopes.iter().enumerate() {
        if !ei_exits
            || body
                .else_if_clauses
                .iter()
                .nth(idx)
                .is_some_and(|ei| exits_via_loop_control(ei.statement))
        {
            surviving_scopes.push(ei_scope);
        }
    }
    if let Some(ref es) = else_scope {
        if !else_exits
            || body
                .else_clause
                .as_ref()
                .is_some_and(|ec| exits_via_loop_control(ec.statement))
        {
            surviving_scopes.push(es);
        }
    } else {
        // No else clause — the pre-if scope is an implicit surviving path.
        // When the then-body does NOT exit, apply inverse condition
        // narrowing so that information from the condition (e.g.
        // `$a["test"] === null` → `$a["test"]` is NOT null in the
        // implicit else path) is reflected in the merge.
        //
        // When the then-body DOES exit (guard clause), skip inverse
        // narrowing here — the dedicated guard clause section below
        // handles it.  Applying it in both places would double-narrow.
        implicit_else_scope = pre_if_scope.clone();
        if !then_exits {
            apply_condition_narrowing_inverse(if_stmt.condition, &mut implicit_else_scope, ctx);
        }
        surviving_scopes.push(&implicit_else_scope);
    }

    if surviving_scopes.is_empty() {
        // All branches exit — theoretically unreachable code after.
        // Keep the pre-if scope.
        *scope = pre_if_scope;
    } else if surviving_scopes.len() == 1 {
        *scope = surviving_scopes[0].clone();
    } else {
        // Merge all surviving scopes.
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

    // Remove synthetic property access keys that were seeded by
    // condition narrowing inside branches.  These represent narrowed
    // types that only hold within specific branches, not after the
    // if/elseif/else block.  This must run BEFORE guard clause
    // narrowing so that guard-clause-narrowed property keys (e.g.
    // `$this->model` narrowed to `Order` after
    // `if (!$this->model instanceof Order) { return; }`) survive
    // into the post-if scope.
    strip_synthetic_property_keys(scope);

    // Guard clause narrowing: when the if body unconditionally exits
    // and there are no elseif/else branches, apply inverse narrowing.
    // This applies to ALL exit types (return, throw, break, continue)
    // because the code after the if in the current scope does not
    // execute in that path.  Break/continue branch scopes are already
    // included in `surviving_scopes` above so their variable
    // assignments are preserved in the merge.
    if enclosing_stmt.span().end.offset < ctx.cursor_offset
        && then_exits
        && body.else_if_clauses.is_empty()
        && body.else_clause.is_none()
    {
        apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
        apply_guard_clause_null_narrowing(if_stmt, scope, ctx);
    }
}

/// Process if with colon-delimited body.
pub(crate) fn process_if_colon_body<'b>(
    if_stmt: &'b If<'b>,
    body: &'b IfColonDelimitedBody<'b>,
    _enclosing_stmt: &'b Statement<'b>,
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

    if cursor_in_then {
        apply_condition_narrowing(if_stmt.condition, scope, ctx);
        walk_body_forward(body.statements.iter(), scope, ctx);
        return;
    }

    // Check elseif clauses.
    for ei in body.else_if_clauses.iter() {
        let ei_start = ei.colon.start.offset;
        let ei_end = ei
            .statements
            .last()
            .map(|s| s.span().end.offset)
            .unwrap_or(ei_start);
        if ctx.cursor_offset >= ei_start && ctx.cursor_offset <= ei_end {
            apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
            apply_condition_narrowing(ei.condition, scope, ctx);
            process_condition_assignment(ei.condition, scope, ctx);
            seed_pass_by_ref_in_condition(ei.condition, scope, ctx);
            walk_body_forward(ei.statements.iter(), scope, ctx);
            return;
        }
    }

    // Check else clause.
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
    apply_condition_narrowing(if_stmt.condition, &mut then_scope, ctx);
    walk_body_forward(body.statements.iter(), &mut then_scope, ctx);

    let mut all_scopes = vec![then_scope];
    for ei in body.else_if_clauses.iter() {
        let mut ei_scope = pre_if_scope.clone();
        // The elseif branch only runs when the if condition and every
        // preceding elseif condition were false, so apply their inverse
        // narrowing before walking this branch.
        apply_condition_narrowing_inverse(if_stmt.condition, &mut ei_scope, ctx);
        for prev_ei in body.else_if_clauses.iter() {
            if std::ptr::eq(prev_ei, ei) {
                break;
            }
            apply_condition_narrowing_inverse(prev_ei.condition, &mut ei_scope, ctx);
        }
        // Record a scope snapshot at the elseif condition boundary so
        // that diagnostic variable lookups inside the condition don't
        // pick up assignments from preceding if/elseif bodies.
        if is_diagnostic_scope_active() {
            record_scope_snapshot(ei.condition.span().start.offset, &ei_scope);
        }
        apply_condition_narrowing(ei.condition, &mut ei_scope, ctx);
        process_condition_assignment(ei.condition, &mut ei_scope, ctx);
        seed_pass_by_ref_in_condition(ei.condition, &mut ei_scope, ctx);
        walk_body_forward(ei.statements.iter(), &mut ei_scope, ctx);
        all_scopes.push(ei_scope);
    }
    if let Some(ref else_clause) = body.else_clause {
        let mut else_scope = pre_if_scope.clone();
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
        all_scopes.push(else_scope);
    } else {
        // No else clause — the pre-if scope is the implicit "all
        // conditions were false" path.  Apply the inverse of the if
        // condition (and every elseif condition) so information from a
        // failed condition is reflected in the merged scope.
        let mut implicit_else_scope = pre_if_scope.clone();
        apply_condition_narrowing_inverse(if_stmt.condition, &mut implicit_else_scope, ctx);
        for ei in body.else_if_clauses.iter() {
            apply_condition_narrowing_inverse(ei.condition, &mut implicit_else_scope, ctx);
        }
        all_scopes.push(implicit_else_scope);
    }

    // Merge all surviving scopes.
    if let Some(first) = all_scopes.first() {
        let mut merged = first.clone();
        for s in &all_scopes[1..] {
            merged.merge_branch(s);
        }
        *scope = merged;
    }
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
    // Build dependency map: assigned_var → set of RHS variables
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();

    for stmt in statements {
        collect_assignment_deps(stmt, &mut deps);
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
        Statement::Foreach(f) => match &f.body {
            ForeachBody::Statement(s) => {
                collect_assignment_deps(s, deps);
            }
            ForeachBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        },
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
    use mago_syntax::cst::variable::Variable;

    if let Expression::Assignment(assign) = expr
        && let Expression::Variable(Variable::Direct(dv)) = assign.lhs
    {
        let lhs_name = bytes_to_str(dv.name).to_string();
        let mut rhs_vars = HashSet::new();
        collect_rhs_variables(assign.rhs, &mut rhs_vars);
        deps.entry(lhs_name).or_default().extend(rhs_vars);
    }
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
/// Unlike `scopes_equal`, this is asymmetric: new variables in
/// `after` that weren't in `before` count as changes, but variables
/// in `before` that aren't in `after` do not (they were just not
/// assigned in the loop body).
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
        let var_annotations = parse_all_var_docblock_annotations(doc_text);
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

    // ── Initial walk (always performed) ─────────────────────────
    let initial_ctx = if assignment_depth > 1 {
        &discovery_ctx
    } else {
        ctx
    };
    match &foreach.body {
        ForeachBody::Statement(inner) => {
            walk_body_forward(std::iter::once(*inner), scope, initial_ctx);
        }
        ForeachBody::ColonDelimited(body) => {
            walk_body_forward(body.statements.iter(), scope, initial_ctx);
        }
    }

    // ── Re-walk iterations (only if types changed) ──────────────
    for iteration in 0..assignment_depth.saturating_sub(1) {
        // Check for changes BEFORE re-walking: compare post-walk
        // scope against the pre-loop scope.  If no variable has a
        // type that differs from what was known before the loop,
        // there's nothing new to propagate — skip the re-walk.
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        // Merge discovered types back into the pre-loop scope and
        // re-bind foreach variables for the next iteration.
        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        match &foreach.target {
            ForeachTarget::Value(val) => {
                bind_foreach_value(val.value, &iter_type, &mut next_scope, ctx);
            }
            ForeachTarget::KeyValue(kv) => {
                bind_foreach_key(kv.key, &iter_type, &mut next_scope, ctx);
                bind_foreach_value(kv.value, &iter_type, &mut next_scope, ctx);
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
        *scope = next_scope;

        // Use the real context on the final iteration so diagnostic
        // snapshots and cursor handling are correct.
        let is_final = iteration + 1 >= assignment_depth.saturating_sub(1);
        let walk_ctx = if is_final { ctx } else { &discovery_ctx };

        match &foreach.body {
            ForeachBody::Statement(inner) => {
                walk_body_forward(std::iter::once(*inner), scope, walk_ctx);
            }
            ForeachBody::ColonDelimited(body) => {
                walk_body_forward(body.statements.iter(), scope, walk_ctx);
            }
        }
    }

    // The iterable might be empty, so the loop body might not execute
    // at all.  Merge with the pre-loop scope.
    let post_loop = scope.clone();
    *scope = pre_loop_scope;
    scope.merge_branch(&post_loop);

    // When the iterable is a non-empty literal array (e.g. `["a", "b",
    // "c"]`), the loop body is guaranteed to execute at least once.
    // The pre-loop sentinel value (e.g. `null` from `$tag = null`) must
    // not survive as a possible post-loop type for the foreach target
    // variable — override it with the post-loop value from the body walk.
    if is_non_empty_array_literal(foreach.expression) {
        let target_var = match &foreach.target {
            ForeachTarget::Value(val) => extract_foreach_var_name(val.value),
            ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.value),
        };
        if let Some(ref vn) = target_var
            && let Some(post_val) = post_loop.locals.get(&ustr::ustr(vn.as_str()))
            && !post_val.is_empty()
        {
            scope.set(vn, post_val.clone());
        }
    }

    leave_loop(loop_depth);
}

/// Resolve the iterable expression's type for a foreach.
pub(crate) fn resolve_foreach_iterable_type<'b>(
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
        let joined = ResolvedType::types_joined(&resolved);
        // Expand type aliases (e.g. `@phpstan-type UserList array<int, User>`)
        // so that `extract_value_type` can see the underlying generic type.
        let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
            &joined,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(joined);
        return Some(expanded);
    }

    // Fallback: for simple `$variable` iterators, check for an inline
    // `/** @var Type $var */` or `@param` annotation near the foreach.
    // This mirrors the backward scanner's `find_iterable_raw_type_in_source`
    // fallback and handles cases where the variable's type comes from a
    // docblock rather than an assignment.
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
            // Expand type aliases on the docblock result too.
            let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
                &docblock_type,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
            .unwrap_or(docblock_type);
            return Some(expanded);
        }
    }

    // Final fallback: resolve the foreach expression as a "subject"
    // through the full resolver pipeline (SubjectExpr::parse →
    // property/method chain resolution).  This mirrors the backward
    // scanner's `resolve_foreach_expression_to_classes` and handles
    // cases like `$this->getItems()` or `self::fetchAll()` where
    // the expression type wasn't captured by scope lookup or
    // resolve_rhs_expression above.
    if let Some(iter_type) = resolve_foreach_expr_via_subject(foreach.expression, scope, ctx) {
        return Some(iter_type);
    }

    None
}

/// Resolve a foreach expression to a `PhpType` by treating it as a
/// subject string and going through the full resolver pipeline.
///
/// This is the forward walker's equivalent of the backward scanner's
/// `resolve_foreach_expression_to_classes`.  It extracts the expression
/// text, calls `resolve_target_classes` to get `ClassInfo` objects, and
/// constructs a `PhpType::Named` from the first resolved class.
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
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__foreach", expr_span.start.offset, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();

    let resolved = crate::completion::resolver::resolve_target_classes(
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
            let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
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
    // Use `fqn()` (not `name`) so that the returned `PhpType::Named`
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

    Some(PhpType::Named(name))
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
///    `@implements` generics.  This mirrors what
///    `try_resolve_foreach_value_type` does in the backward scanner.
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
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &vt,
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

            // Strategy 2: class-based fallback for bare collection names.
            let element_via_class = resolve_iterable_element_via_class(it, ctx);
            if let Some(element_type) = element_via_class
                && !is_unsubstituted_template_param(&element_type)
            {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
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
            }

            // Strategy 3: union type fallback — try each member individually.
            // When the iterable is a union like `ProductCollection|Product`,
            // neither `extract_value_type` nor `resolve_iterable_element_via_class`
            // works on the union as a whole.  Walk each member and use the
            // first one that yields an element type.
            if let PhpType::Union(members) = it {
                for member in members {
                    // Try extract_value_type on each member (handles generic collections).
                    if let Some(vt) = member.extract_value_type(false) {
                        let resolved =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
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
                    if let Some(element_type) = resolve_iterable_element_via_class(member, ctx)
                        && !is_unsubstituted_template_param(&element_type)
                    {
                        let resolved =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
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
            // Try direct value type extraction first (handles tuple-style
            // shapes by unioning their positional value types).
            if let Some(vt) = it.iterable_element_type() {
                return Some(vt);
            }
            // Try class-based iterable element extraction.
            if let Some(et) = resolve_iterable_element_via_class(it, ctx)
                && !is_unsubstituted_template_param(&et)
            {
                return Some(et);
            }
            // Try union members individually.
            if let PhpType::Union(members) = it {
                for member in members {
                    if let Some(vt) = member.extract_value_type(false) {
                        return Some(vt.clone());
                    }
                    if let Some(et) = resolve_iterable_element_via_class(member, ctx)
                        && !is_unsubstituted_template_param(&et)
                    {
                        return Some(et);
                    }
                }
            }
            None
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
                    _ => continue,
                };

                // Try shape key lookup first, then fall back to generic element type.
                let resolved_type = shape_key
                    .as_ref()
                    .and_then(|k| elem_type.shape_value_type(k).cloned())
                    .or_else(|| elem_type.extract_value_type(true).cloned());

                if let Some(ref vt) = resolved_type {
                    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
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

/// Extract a string key from a foreach destructuring key expression.
///
/// Handles string literals (`'user'`, `"user"`) and integer literals.
pub(crate) fn extract_foreach_destr_key(key_expr: &Expression<'_>) -> Option<String> {
    match key_expr {
        Expression::Literal(Literal::String(lit_str)) => lit_str
            .value
            .map(|v| bytes_to_str(v).to_string())
            .or_else(|| {
                let raw = bytes_to_str(lit_str.raw).to_string();
                Some(raw.trim_matches('\'').trim_matches('"').to_string())
            }),
        Expression::Literal(Literal::Integer(lit_int)) => {
            Some(bytes_to_str(lit_int.raw).to_string())
        }
        _ => None,
    }
}

/// Check whether a `PhpType` looks like an unsubstituted template
/// parameter (e.g. `TValue`, `TKey`, `TModel`).  These are bare named
/// types whose name starts with `T` followed by an uppercase letter
/// and are not known PHP built-in types.
pub(crate) fn is_unsubstituted_template_param(ty: &PhpType) -> bool {
    let name = match ty {
        PhpType::Named(n) => n.as_str(),
        _ => return false,
    };
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'T' && bytes[1].is_ascii_uppercase()
}

/// Resolve the element type of an iterable via class inheritance.
///
/// When the iterable type is a bare class name (e.g. `OrderProductCollection`),
/// this resolves it to `ClassInfo`, merges the full inheritance chain, and
/// extracts the element type from `@extends` / `@implements` generics using
/// [`extract_iterable_element_type_from_class`].
pub(crate) fn resolve_iterable_element_via_class(
    iter_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // Accept bare class names, whether or not wrapped in `Nullable` (e.g.
    // `?SimpleXMLElement`, the return type of `SimpleXMLElement::children()`).
    // `base_name` unwraps `Nullable`/`Generic` to the underlying class name.
    // Bare generic types like `Collection<int, User>` are handled by
    // extract_value_type above, so this only needs the name for the
    // `class_loader` fallback below; `type_hint_to_classes_typed` handles
    // the full (possibly nullable) type itself.
    let class_name = iter_type.base_name()?;

    // Resolve the class name to ClassInfo.
    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        iter_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );

    if classes.is_empty() {
        // Try direct class loader as fallback (handles FQN names).
        let cls = (ctx.class_loader)(class_name)?;
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        return super::super::foreach_resolution::extract_iterable_element_type_from_class(
            &merged,
            ctx.class_loader,
        );
    }

    for cls in &classes {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let element_type =
            super::super::foreach_resolution::extract_iterable_element_type_from_class(
                &merged,
                ctx.class_loader,
            );
        if let Some(ref et) = element_type {
            // When the extracted type is an unsubstituted template parameter
            // (e.g. `TModel`), resolve it through the class's template bounds
            // (e.g. `@template TModel of BlogAuthor` → `BlogAuthor`).
            if let Some(name) = et.base_name()
                && merged
                    .template_params
                    .iter()
                    .any(|p| p.as_ref() as &str == name)
                && let Some(bound) = merged.template_param_bounds.get(&crate::atom::atom(name))
            {
                return Some(bound.clone());
            }
            return element_type;
        }
    }

    None
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
        if let Some(it) = iter_type {
            // Strategy 1: extract from the type's own generic parameters.
            let key_php_type = it.extract_key_type(false);
            if let Some(kt) = key_php_type {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    kt,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, kt.clone()),
                    );
                } else {
                    scope.set(&var_name, vec![ResolvedType::from_type_string(kt.clone())]);
                }
                return;
            }

            // Strategy 2: class-based fallback for bare collection names.
            // When the iterable is `PhpType::Named("Finder")` (no generics),
            // look at the class's implements_generics / extends_generics to
            // find the key type (e.g. IteratorAggregate<non-empty-string, SplFileInfo>).
            if let Some(key_type) = resolve_iterable_key_via_class(it, ctx)
                && !is_unsubstituted_template_param(&key_type)
            {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &key_type,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, key_type),
                    );
                } else {
                    scope.set(&var_name, vec![ResolvedType::from_type_string(key_type)]);
                }
                return;
            }
        }
        // Default: key is int|string.
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::Union(vec![
                PhpType::int(),
                PhpType::string(),
            ]))],
        );
    }
}

/// Resolve the iterable **key** type from a class's `implements_generics`
/// / `extends_generics`.  Mirrors `resolve_iterable_element_via_class`.
pub(crate) fn resolve_iterable_key_via_class(
    iter_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // See `resolve_iterable_element_via_class`: unwrap `Nullable`/`Generic`
    // via `base_name` so `?SimpleXMLElement`-style iterable types resolve.
    let class_name = iter_type.base_name()?;

    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        iter_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );

    if classes.is_empty() {
        let cls = (ctx.class_loader)(class_name)?;
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        return super::super::foreach_resolution::extract_iterable_key_type_from_class(
            &merged,
            ctx.class_loader,
        );
    }

    for cls in &classes {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let key_type = super::super::foreach_resolution::extract_iterable_key_type_from_class(
            &merged,
            ctx.class_loader,
        );
        if let Some(ref kt) = key_type {
            if let Some(name) = kt.base_name()
                && merged
                    .template_params
                    .iter()
                    .any(|p| p.as_ref() as &str == name)
                && let Some(bound) = merged.template_param_bounds.get(&crate::atom::atom(name))
            {
                return Some(bound.clone());
            }
            return key_type;
        }
    }

    None
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
    record_and_chain_snapshots(while_stmt.condition, scope, ctx);
    record_or_chain_snapshots(while_stmt.condition, scope, ctx);

    let pre_loop_scope = scope.clone();

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

    // Assignment in condition: `while ($x = expr())`
    process_condition_assignment(while_stmt.condition, scope, ctx);

    // Pass-by-reference in condition: `while (preg_match(..., $matches))`
    seed_pass_by_ref_in_condition(while_stmt.condition, scope, ctx);

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

    // ── Initial walk (always performed) ─────────────────────────
    let initial_ctx = if assignment_depth > 1 {
        &discovery_ctx
    } else {
        ctx
    };
    match &while_stmt.body {
        WhileBody::Statement(inner) => {
            walk_body_forward(std::iter::once(*inner), scope, initial_ctx);
        }
        WhileBody::ColonDelimited(body) => {
            walk_body_forward(body.statements.iter(), scope, initial_ctx);
        }
    }

    // ── Re-walk iterations (only if types changed) ──────────────
    for iteration in 0..assignment_depth.saturating_sub(1) {
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        apply_condition_narrowing(while_stmt.condition, &mut next_scope, ctx);
        process_condition_assignment(while_stmt.condition, &mut next_scope, ctx);
        seed_pass_by_ref_in_condition(while_stmt.condition, &mut next_scope, ctx);
        *scope = next_scope;

        let is_final = iteration + 1 >= assignment_depth.saturating_sub(1);
        let walk_ctx = if is_final { ctx } else { &discovery_ctx };

        match &while_stmt.body {
            WhileBody::Statement(inner) => {
                walk_body_forward(std::iter::once(*inner), scope, walk_ctx);
            }
            WhileBody::ColonDelimited(body) => {
                walk_body_forward(body.statements.iter(), scope, walk_ctx);
            }
        }
    }

    // When the cursor is inside the loop body (completion path), keep
    // the scope with condition narrowing applied.  The post-loop
    // merge would erase the narrowing (since the loop might not execute),
    // but the cursor IS inside the body, so the condition is true.
    if cursor_in_body && !is_diagnostic_scope_active() {
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
        process_condition_assignment(cond_expr, scope, ctx);
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

    let pre_loop_scope = scope.clone();

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
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // ── Initial walk (always performed) ─────────────────────────
    let initial_ctx = if assignment_depth > 1 {
        &discovery_ctx
    } else {
        ctx
    };
    match &for_stmt.body {
        ForBody::Statement(inner) => {
            walk_body_forward(std::iter::once(*inner), scope, initial_ctx);
        }
        ForBody::ColonDelimited(body) => {
            walk_body_forward(body.statements.iter(), scope, initial_ctx);
        }
    }

    // ── Re-walk iterations (only if types changed) ──────────────
    for iteration in 0..assignment_depth.saturating_sub(1) {
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        for init_expr in for_stmt.initializations.iter() {
            process_assignment_expr(init_expr, &mut next_scope, ctx);
        }
        *scope = next_scope;

        let is_final = iteration + 1 >= assignment_depth.saturating_sub(1);
        let walk_ctx = if is_final { ctx } else { &discovery_ctx };

        match &for_stmt.body {
            ForBody::Statement(inner) => {
                walk_body_forward(std::iter::once(*inner), scope, walk_ctx);
            }
            ForBody::ColonDelimited(body) => {
                walk_body_forward(body.statements.iter(), scope, walk_ctx);
            }
        }
    }

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

    // The loop body might not execute at all (condition false on
    // first check), so merge with the pre-loop scope.
    let post_loop = scope.clone();
    *scope = pre_loop_scope;
    scope.merge_branch(&post_loop);

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

    // ── Assignment-depth-bounded loop iteration ─────────────────
    let body_stmts: Vec<&Statement<'b>> = vec![dw.statement];
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // ── Initial walk (always performed) ─────────────────────────
    walk_body_forward(std::iter::once(dw.statement), scope, ctx);

    // ── Re-walk iterations (only if types changed) ──────────────
    for _iteration in 0..assignment_depth.saturating_sub(1) {
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        process_condition_assignment(dw.condition, &mut next_scope, ctx);
        seed_pass_by_ref_in_condition(dw.condition, &mut next_scope, ctx);
        *scope = next_scope;

        walk_body_forward(std::iter::once(dw.statement), scope, ctx);
    }

    // After the do-while loop, the condition evaluated to false (that's
    // why the loop exited).  Apply the inverse of the condition to narrow
    // types.  For example: `do { $a = getA(); } while ($a !== null);`
    // => after loop, $a is null.
    apply_condition_narrowing_inverse(dw.condition, scope, ctx);

    leave_loop(loop_depth);
}

/// Process a `try-catch-finally` statement.
pub(crate) fn process_try<'b>(
    try_stmt: &'b Try<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let pre_try_scope = scope.clone();

    // Check if cursor is inside the try body.
    let try_body_span = try_stmt.block.span();
    let cursor_in_try = ctx.cursor_offset >= try_body_span.start.offset
        && ctx.cursor_offset <= try_body_span.end.offset;

    if cursor_in_try {
        // Walk only the try body.
        walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
        return;
    }

    // Check if cursor is inside a catch block.
    for catch in try_stmt.catch_clauses.iter() {
        let catch_span = catch.block.span();
        if ctx.cursor_offset >= catch_span.start.offset
            && ctx.cursor_offset <= catch_span.end.offset
        {
            // Bind the caught exception variable.
            if let Some(ref var) = catch.variable {
                let var_name = bytes_to_str(var.name).to_string();
                let parsed_hint = extract_hint_type(&catch.hint);
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &parsed_hint,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                let exception_types = ResolvedType::from_classes_with_hint(resolved, parsed_hint);
                // Merge pre-try scope (since the exception could have
                // been thrown at any point in the try body) with the
                // catch variable.
                *scope = pre_try_scope.clone();
                if !exception_types.is_empty() {
                    scope.set(&var_name, exception_types);
                }
            } else {
                *scope = pre_try_scope.clone();
            }
            walk_body_forward(catch.block.statements.iter(), scope, ctx);
            return;
        }
    }

    // Check if cursor is inside the finally block.
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
        if let Some(ref var) = catch.variable {
            let var_name = bytes_to_str(var.name).to_string();
            let parsed_hint = extract_hint_type(&catch.hint);
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                &parsed_hint,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            let exception_types = ResolvedType::from_classes_with_hint(resolved, parsed_hint);
            if !exception_types.is_empty() {
                catch_scope.set(&var_name, exception_types);
            }
        }
        walk_body_forward(catch.block.statements.iter(), &mut catch_scope, ctx);
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

    let mut branch_scopes: Vec<ScopeState> = Vec::new();
    let mut has_default = false;

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

        let mut case_scope = pre_switch_scope.clone();
        walk_body_forward(accumulated_stmts.iter().copied(), &mut case_scope, ctx);
        branch_scopes.push(case_scope);
        accumulated_stmts.clear();
    }

    // Handle trailing fall-through cases (empty cases at the end).
    if !accumulated_stmts.is_empty() {
        let mut case_scope = pre_switch_scope.clone();
        walk_body_forward(accumulated_stmts.iter().copied(), &mut case_scope, ctx);
        branch_scopes.push(case_scope);
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
