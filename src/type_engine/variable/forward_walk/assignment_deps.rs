//! Assignment-dependency analysis that sizes a loop's fixed point.
//!
//! A cheap AST walk (no type resolution) that records which variables each
//! assignment reads, so the loop walker knows how many passes it takes for
//! a type to propagate along the longest chain.

use super::*;
use std::collections::{HashMap, HashSet};

use crate::atom::bytes_to_str;

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
    // `$i++` / `--$i` rewrites the variable from its own previous value
    // (the literal `0` a counter starts at becomes `int`), so it is a
    // self-edge just like an indexed write.
    if let Some(operand) = increment_decrement_operand(expr) {
        let mut targets = HashSet::new();
        collect_assignment_target_vars(operand, &mut targets);
        for target in targets {
            deps.entry(target.clone()).or_default().insert(target);
        }
        return;
    }

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

fn increment_decrement_operand<'a>(expr: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
    use mago_syntax::cst::unary::UnaryPrefixOperator;

    match expr {
        Expression::UnaryPostfix(postfix) => Some(postfix.operand),
        Expression::UnaryPrefix(prefix)
            if matches!(
                prefix.operator,
                UnaryPrefixOperator::PreIncrement(_) | UnaryPrefixOperator::PreDecrement(_)
            ) =>
        {
            Some(prefix.operand)
        }
        _ => None,
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
