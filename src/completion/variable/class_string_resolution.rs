/// Class-string variable resolution.
///
/// Resolves variables that hold a class-string value (e.g. `$cls = User::class`)
/// to the referenced `ClassInfo` instances.  This is used when the access kind
/// is `::` (`$cls::`) so that static members are offered instead of string
/// methods.
///
/// Handles simple assignments, match expressions, ternary / null-coalescing
/// branches, and `self` / `static` / `parent` keywords.
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::types::ClassInfo;
use crate::util::{resolve_class_keyword, short_name};

use crate::completion::conditional_resolution::extract_class_string_from_expr;
use crate::completion::resolver::{Loaders, VarResolutionCtx};

/// Resolve a `$variable` that holds a class-string (e.g. `$cls = User::class`)
/// to the referenced class(es).
///
/// This is used when the access kind is `::` (`$cls::`) — instead of
/// resolving the variable to its *value type* (`string`), we resolve it
/// to the *referenced class* so that static members are offered.
///
/// Handles simple assignments (`$cls = User::class`), match expressions
/// (`$cls = match(...) { ... => A::class, ... => B::class }`), and
/// ternary / null-coalescing branches.
pub(in crate::completion) fn resolve_class_string_targets(
    var_name: &str,
    current_class: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    content: &str,
    cursor_offset: u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<ClassInfo> {
    with_parsed_program(
        content,
        "resolve_class_string_targets",
        |program, _content| {
            let ctx = VarResolutionCtx {
                var_name,
                current_class,
                all_classes,
                content,
                cursor_offset,
                class_loader,
                loaders: Loaders::default(),
                resolved_class_cache: crate::virtual_members::active_resolved_class_cache(),
                enclosing_return_type: None,
                top_level_scope: None,
                branch_aware: false,
                match_arm_narrowing: Default::default(),
                scope_var_resolver: None,
            };
            resolve_class_string_in_statements(program.statements.iter(), &ctx)
        },
    )
}

/// Walk statements to find class-string assignments to the target variable.
fn resolve_class_string_in_statements<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ClassInfo> {
    let stmts: Vec<&Statement> = statements.collect();

    // Check class bodies first (same pattern as resolve_variable_in_statements).
    for &stmt in &stmts {
        match stmt {
            Statement::Class(class) => {
                let start = class.left_brace.start.offset;
                let end = class.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    return resolve_class_string_in_members(class.members.iter(), ctx);
                }
            }
            Statement::Interface(iface) => {
                let start = iface.left_brace.start.offset;
                let end = iface.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    return resolve_class_string_in_members(iface.members.iter(), ctx);
                }
            }
            Statement::Enum(enum_def) => {
                let start = enum_def.left_brace.start.offset;
                let end = enum_def.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    return resolve_class_string_in_members(enum_def.members.iter(), ctx);
                }
            }
            Statement::Trait(trait_def) => {
                let start = trait_def.left_brace.start.offset;
                let end = trait_def.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    return resolve_class_string_in_members(trait_def.members.iter(), ctx);
                }
            }
            Statement::Namespace(ns) => {
                let results = resolve_class_string_in_statements(ns.statements().iter(), ctx);
                if !results.is_empty() {
                    return results;
                }
            }
            Statement::Function(func) => {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                if ctx.cursor_offset >= body_start && ctx.cursor_offset <= body_end {
                    let mut results = Vec::new();
                    walk_class_string_assignments(func.body.statements.iter(), ctx, &mut results);
                    return results;
                }
            }
            _ => {}
        }
    }

    // Top-level code.
    let mut results = Vec::new();
    walk_class_string_assignments(stmts.into_iter(), ctx, &mut results);
    results
}

/// Resolve class-string assignments inside class-like members.
fn resolve_class_string_in_members<'b>(
    members: impl Iterator<Item = &'b ClassLikeMember<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ClassInfo> {
    for member in members {
        if let ClassLikeMember::Method(method) = member {
            let body = match &method.body {
                MethodBody::Concrete(body) => body,
                _ => continue,
            };
            let start = body.left_brace.start.offset;
            let end = body.right_brace.end.offset;
            if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                let mut results = Vec::new();
                walk_class_string_assignments(body.statements.iter(), ctx, &mut results);
                return results;
            }
        }
    }
    vec![]
}

/// Walk statements collecting class names from `$var = Foo::class` assignments.
fn walk_class_string_assignments<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    // Track variables assigned an array literal of `::class` entries
    // (e.g. `$repos = [Foo::class, Bar::class]`) so that a later
    // `foreach ($repos as $repository)` can resolve `$repository` to the
    // union of element classes.  Assignments precede the foreach in the
    // statement stream, so recording them as we go makes the mapping
    // available by the time the foreach is processed.
    let mut array_class_vars: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for stmt in statements {
        if stmt.span().start.offset >= ctx.cursor_offset {
            continue;
        }
        // Record `$var = [A::class, B::class]` array-literal assignments.
        if let Statement::Expression(expr_stmt) = stmt
            && let Expression::Assignment(assignment) = expr_stmt.expression
            && assignment.operator.is_assign()
            && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
        {
            let names = extract_class_string_names_from_array(assignment.rhs);
            if !names.is_empty() {
                array_class_vars.insert(bytes_to_str(dv.name).to_string(), names);
            }
        }
        match stmt {
            Statement::Expression(expr_stmt) => {
                check_class_string_assignment(expr_stmt.expression, ctx, results);
            }
            Statement::Foreach(foreach) => {
                // Check if the foreach value variable matches our target
                // and the iterated expression is an array of ::class literals.
                let value_expr = foreach.target.value();
                let value_name = match value_expr {
                    Expression::Variable(Variable::Direct(dv)) => {
                        Some(bytes_to_str(dv.name).to_string())
                    }
                    _ => None,
                };
                if let Some(name) = value_name
                    && name == ctx.var_name
                {
                    // Extract class names from the iterated expression.
                    // The iterable may be an inline array literal
                    // (`[Page::class, CustomPage::class]`) or a variable
                    // that was assigned one earlier (`$pages = [...]`).
                    let mut class_names = extract_class_string_names_from_array(foreach.expression);
                    if class_names.is_empty()
                        && let Expression::Variable(Variable::Direct(dv)) = foreach.expression
                        && let Some(names) = array_class_vars.get(bytes_to_str(dv.name))
                    {
                        class_names = names.clone();
                    }
                    if !class_names.is_empty() {
                        results.clear();
                        for cn in class_names {
                            let resolved_name = if let Some(resolved) =
                                resolve_class_keyword(&cn, Some(ctx.current_class))
                            {
                                resolved
                            } else {
                                cn
                            };
                            let lookup = short_name(&resolved_name);
                            if let Some(cls) = ctx.all_classes.iter().find(|c| c.name == lookup) {
                                ClassInfo::push_unique(results, ClassInfo::clone(cls));
                            } else if let Some(cls) = (ctx.class_loader)(&resolved_name) {
                                ClassInfo::push_unique(results, Arc::unwrap_or_clone(cls));
                            }
                        }
                    }
                }
                // Also walk the foreach body for nested assignments.
                let body_stmts: Vec<&Statement> = match &foreach.body {
                    mago_syntax::cst::ForeachBody::Statement(s) => vec![s],
                    mago_syntax::cst::ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().collect()
                    }
                };
                walk_class_string_assignments(body_stmts.into_iter(), ctx, results);
            }
            // Descend into nested control-flow blocks so that an assignment
            // inside a braced foreach/if/while/for/switch/try body (the common
            // case: `foreach (...) { $cls = Foo::class; }`) is still found.
            other => {
                let nested = collect_nested_statements(other);
                if !nested.is_empty() {
                    walk_class_string_assignments(nested.into_iter(), ctx, results);
                }
            }
        }
    }
}

/// Collect the directly-nested statements of a control-flow statement so the
/// class-string walker can recurse into braced blocks.  Returns an empty list
/// for statements that carry no nested block (the walker then does nothing).
fn collect_nested_statements<'b>(stmt: &'b Statement<'b>) -> Vec<&'b Statement<'b>> {
    let mut out: Vec<&Statement> = Vec::new();
    match stmt {
        Statement::Block(block) => out.extend(block.statements.iter()),
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                out.push(body.statement);
                for clause in body.else_if_clauses.iter() {
                    out.push(clause.statement);
                }
                if let Some(else_clause) = &body.else_clause {
                    out.push(else_clause.statement);
                }
            }
            IfBody::ColonDelimited(body) => {
                out.extend(body.statements.iter());
                for clause in body.else_if_clauses.iter() {
                    out.extend(clause.statements.iter());
                }
                if let Some(else_clause) = &body.else_clause {
                    out.extend(else_clause.statements.iter());
                }
            }
        },
        Statement::While(while_stmt) => match &while_stmt.body {
            WhileBody::Statement(inner) => out.push(inner),
            WhileBody::ColonDelimited(body) => out.extend(body.statements.iter()),
        },
        Statement::DoWhile(dw) => out.push(dw.statement),
        Statement::For(for_stmt) => match &for_stmt.body {
            ForBody::Statement(inner) => out.push(inner),
            ForBody::ColonDelimited(body) => out.extend(body.statements.iter()),
        },
        Statement::Switch(switch) => {
            for case in switch.body.cases().iter() {
                match case {
                    SwitchCase::Expression(c) => out.extend(c.statements.iter()),
                    SwitchCase::Default(c) => out.extend(c.statements.iter()),
                }
            }
        }
        Statement::Try(try_stmt) => {
            out.extend(try_stmt.block.statements.iter());
            for catch in try_stmt.catch_clauses.iter() {
                out.extend(catch.block.statements.iter());
            }
            if let Some(finally) = &try_stmt.finally_clause {
                out.extend(finally.block.statements.iter());
            }
        }
        _ => {}
    }
    out
}

/// Check if an expression is an assignment of a `::class` literal
/// to the target variable, and if so, resolve the class.
fn check_class_string_assignment(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    let Expression::Assignment(assignment) = expr else {
        return;
    };
    if !assignment.operator.is_assign() {
        return;
    }
    let lhs_name = match assignment.lhs {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        _ => return,
    };
    if lhs_name != ctx.var_name {
        return;
    }

    let class_names = extract_class_string_names(assignment.rhs);
    // Clear previous results — the last unconditional assignment wins.
    results.clear();
    for name in class_names {
        let resolved_name =
            if let Some(resolved) = resolve_class_keyword(&name, Some(ctx.current_class)) {
                resolved
            } else {
                name
            };
        let lookup = short_name(&resolved_name);
        if let Some(cls) = ctx.all_classes.iter().find(|c| c.name == lookup) {
            ClassInfo::push_unique(results, ClassInfo::clone(cls));
        } else if let Some(cls) = (ctx.class_loader)(&resolved_name) {
            ClassInfo::push_unique(results, Arc::unwrap_or_clone(cls));
        }
    }
}

/// Extract class names from `::class` expressions, recursing into
/// match arms, ternary branches, null-coalescing, and parenthesized
/// expressions.
fn extract_class_string_names(expr: &Expression<'_>) -> Vec<String> {
    if let Some(name) = extract_class_string_from_expr(expr) {
        return vec![name];
    }
    match expr {
        Expression::Parenthesized(p) => extract_class_string_names(p.expression),
        Expression::Match(match_expr) => {
            let mut names = Vec::new();
            for arm in match_expr.arms.iter() {
                names.extend(extract_class_string_names(arm.expression()));
            }
            names
        }
        Expression::Conditional(cond) => {
            let mut names = Vec::new();
            let then_expr = cond.then.unwrap_or(cond.condition);
            names.extend(extract_class_string_names(then_expr));
            names.extend(extract_class_string_names(cond.r#else));
            names
        }
        Expression::Binary(binary) if binary.operator.is_null_coalesce() => {
            let mut names = Vec::new();
            names.extend(extract_class_string_names(binary.lhs));
            names.extend(extract_class_string_names(binary.rhs));
            names
        }
        _ => vec![],
    }
}

/// Extract class names from array elements that are `::class` literals.
///
/// Handles `[Page::class, CustomPage::class]` and similar array
/// expressions used as foreach iterables.
fn extract_class_string_names_from_array(expr: &Expression<'_>) -> Vec<String> {
    match expr {
        Expression::Array(array) => {
            let mut names = Vec::new();
            for item in array.elements.iter() {
                if let ArrayElement::Value(val) = item {
                    names.extend(extract_class_string_names(val.value));
                }
            }
            names
        }
        Expression::LegacyArray(array) => {
            let mut names = Vec::new();
            for item in array.elements.iter() {
                if let ArrayElement::Value(val) = item {
                    names.extend(extract_class_string_names(val.value));
                }
            }
            names
        }
        _ => vec![],
    }
}
