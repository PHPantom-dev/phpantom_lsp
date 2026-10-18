//! Seeding a function's `static $var;` locals.
//!
//! A `static` local is the one variable in a body whose value does not come
//! from anything the walker passes on its way down: it keeps whatever an
//! *earlier* call left there, and that assignment can sit in a branch this
//! call never reaches.
//!
//! ```php
//! function info(?Configuration $config = null) {
//!     static $lastConfig;
//!     if ($config !== null) {
//!         $lastConfig = $config;   // the call that stores it …
//!         return null;
//!     }
//!     $config = $lastConfig ?: new Configuration();  // … is never this one
//! }
//! ```
//!
//! Walking top to bottom reads `$lastConfig` as never assigned, because the
//! only assignment to it is behind a `return` in the other branch.  The
//! declaration is therefore seeded up front from the union of its own
//! initialiser and every assignment to the name anywhere in the body, which
//! is the set of values a `static` can actually hold on entry.

use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::parser::for_each_nested_statement_list;
use crate::types::ResolvedType;

use super::ScopeState;
use super::walk_ctx::ForwardWalkCtx;

/// How deep to look for `static` declarations and their assignments.
///
/// Both walks descend through the block structure of one function body,
/// which real code keeps shallow; the bound is a backstop, not a budget.
const MAX_BODY_DEPTH: u8 = 24;

/// Seed every `static $var;` a body declares with the union of its
/// initialiser and every assignment the body makes to it.
///
/// Call after the parameters are seeded: an assignment like
/// `$lastConfig = $config;` is resolved against the scope as it stands, so
/// the parameter it copies has to be in there already.
///
/// Costs nothing for a body with no `static` declaration, which is nearly
/// all of them: the first walk stops at the statement list it was given
/// without resolving anything.
pub(crate) fn seed_static_locals<'b>(
    scope: &mut ScopeState,
    body: &[&'b Statement<'b>],
    ctx: &ForwardWalkCtx<'_>,
) {
    let mut names: Vec<&'b str> = Vec::new();
    let mut initialisers: Vec<(&'b str, &'b Expression<'b>)> = Vec::new();
    for stmt in body {
        collect_static_declarations(stmt, &mut names, &mut initialisers, 0);
    }
    if names.is_empty() {
        return;
    }

    let mut seeds: Vec<(&str, Vec<ResolvedType>)> =
        names.iter().map(|name| (*name, Vec::new())).collect();

    for (name, value) in &initialisers {
        add_expression_types(&mut seeds, name, value, scope, ctx);
    }
    for stmt in body {
        collect_assigned_types(stmt, &mut seeds, scope, ctx, 0);
    }

    for (name, types) in seeds {
        if types.is_empty() {
            // Declared but never given a value the walker can name: the
            // variable is still in scope (it is `null` until assigned), so
            // record it as known-but-untyped rather than leaving reads of
            // it to look undefined.
            scope.set_empty(name);
        } else {
            scope.set(name, types);
        }
    }
}

/// Record every `static $a, $b = expr;` declaration in a statement tree.
fn collect_static_declarations<'b>(
    stmt: &'b Statement<'b>,
    names: &mut Vec<&'b str>,
    initialisers: &mut Vec<(&'b str, &'b Expression<'b>)>,
    depth: u8,
) {
    if depth > MAX_BODY_DEPTH {
        return;
    }
    if let Statement::Static(static_stmt) = stmt {
        for item in static_stmt.items.iter() {
            let name = bytes_to_str(item.variable().name);
            if !names.contains(&name) {
                names.push(name);
            }
            if let Some(value) = item.value() {
                initialisers.push((name, value));
            }
        }
        return;
    }
    for_each_nested_statement_list(stmt, |nested| {
        for inner in nested {
            collect_static_declarations(inner, names, initialisers, depth + 1);
        }
    });
}

/// Add the types of every `$name = expr` in a statement tree to `seeds`,
/// for the names `seeds` tracks.
fn collect_assigned_types<'b>(
    stmt: &'b Statement<'b>,
    seeds: &mut [(&str, Vec<ResolvedType>)],
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    depth: u8,
) {
    if depth > MAX_BODY_DEPTH {
        return;
    }
    if let Statement::Expression(expr_stmt) = stmt
        && let Expression::Assignment(assignment) = expr_stmt.expression
        && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
    {
        let name = bytes_to_str(dv.name);
        if seeds.iter().any(|(seeded, _)| *seeded == name) {
            add_expression_types(seeds, name, assignment.rhs, scope, ctx);
        }
    }
    for_each_nested_statement_list(stmt, |nested| {
        for inner in nested {
            collect_assigned_types(inner, seeds, scope, ctx, depth + 1);
        }
    });
}

/// Resolve `expr` against the seeding scope and union it into `name`'s entry.
fn add_expression_types(
    seeds: &mut [(&str, Vec<ResolvedType>)],
    name: &str,
    expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Some(entry) = seeds.iter_mut().find(|(seeded, _)| *seeded == name) else {
        return;
    };
    let resolved = super::assignment::resolve_rhs_with_scope(expr, scope, ctx);
    if !resolved.is_empty() {
        ResolvedType::extend_unique(&mut entry.1, resolved);
    }
}
