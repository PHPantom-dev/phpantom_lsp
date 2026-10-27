//! A lightweight PHP expression walker for the Laravel modules: visiting
//! every expression of a file, a program or a body, and the expressions
//! that run before a cursor in its scope.

use std::ops::ControlFlow;

use mago_span::{HasSpan, Span};
use mago_syntax::cst::*;

/// Parse `content` as PHP and call `visitor` for every expression node
/// (pre-order, depth-first).  Used by navigation modules to find specific
/// function and static-method call patterns without duplicating the full
/// statement-walker boilerplate.
///
/// The visitor returns `ControlFlow::Continue(())` to keep walking or
/// `ControlFlow::Break(())` to stop early (e.g. after finding a match).
pub(crate) fn walk_all_php_expressions(
    content: &str,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    crate::parser::with_parsed_program(content, "walk_all_php_expressions", |program, _| {
        walk_program_expressions(program, visitor);
    });
}

/// Like [`walk_all_php_expressions`], but for a `Program` the caller has
/// already parsed and needs to keep around afterwards (e.g. to resolve a
/// local variable back to its assignment).
pub(crate) fn walk_program_expressions(
    program: &Program<'_>,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    for stmt in program.statements.iter() {
        if walk_stmt_exprs(stmt, visitor).is_break() {
            return;
        }
    }
}

/// Like [`walk_program_expressions`], but rooted at a single expression the
/// caller already holds (e.g. one argument of a call it matched), descending
/// into closure and arrow-function bodies the same way.
pub(crate) fn walk_expression_tree(
    expr: &Expression<'_>,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    let _ = walk_expr_depth(expr, visitor);
}

/// Like [`walk_program_expressions`], but rooted at one block the caller
/// already holds (a method or closure body).
pub(crate) fn walk_block_expressions(
    block: &Block<'_>,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    for stmt in block.statements.iter() {
        if walk_stmt_exprs(stmt, visitor).is_break() {
            return;
        }
    }
}

// ─── Scope walking ──────────────────────────────────────────────────────────

/// Whether `span` covers `offset`, inclusive at both ends.
pub(in crate::virtual_members::laravel) fn covers(span: Span, offset: u32) -> bool {
    offset >= span.start.offset && offset <= span.end.offset
}

/// The outermost function-like body containing `offset`.
///
/// Outermost rather than innermost so that a construct in a controller
/// action still applies inside a closure nested in that action — "earlier
/// in the same method" covers everything the method wraps. A sibling
/// method's body never contains the offset, so a lookup stays scoped to the
/// one being edited.
///
/// Spans nest, so only the cursor's own ancestors are descended into: a
/// subtree that does not cover the offset cannot hold the body that does.
/// That keeps the search proportional to nesting depth rather than to file
/// size.
pub(in crate::virtual_members::laravel) fn enclosing_body<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    offset: u32,
) -> Option<Node<'ast, 'arena>> {
    let body = match node {
        Node::Method(m) => match &m.body {
            MethodBody::Concrete(block) => Some(Node::Block(block)),
            MethodBody::Abstract(_) => None,
        },
        Node::Function(f) => Some(Node::Block(&f.body)),
        Node::Closure(c) => Some(Node::Block(&c.body)),
        _ => None,
    };
    if let Some(body) = body
        && covers(body.span(), offset)
    {
        return Some(body);
    }

    let mut found = None;
    node.visit_children(|child| {
        if found.is_none() && covers(child.span(), offset) {
            found = enclosing_body(child, offset);
        }
    });
    found
}

/// Hand every node of `node`'s subtree that starts before `cursor` to
/// `visit`.
///
/// Callers are looking for a construct that *completes* before the cursor,
/// and one cannot end before the cursor without starting before it, so
/// subtrees that begin at or after the cursor are skipped rather than walked.
pub(in crate::virtual_members::laravel) fn walk_before_cursor<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    cursor: u32,
    visit: &mut impl FnMut(Node<'ast, 'arena>),
) {
    visit(node);
    node.visit_children(|child| {
        if child.span().start.offset < cursor {
            walk_before_cursor(child, cursor, visit);
        }
    });
}

/// Hand every node of `node`'s subtree that starts before `cursor` to
/// `visit`, stopping at anything that has variables of its own.
///
/// The file-scope counterpart to [`walk_before_cursor`], for an offset no
/// function-like body encloses: a route file registers its routes at the top
/// level of the script, so a `$path = …` written inside a function or closure
/// further up the file says nothing about the value in force there.
pub(in crate::virtual_members::laravel) fn walk_file_scope_before_cursor<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    cursor: u32,
    visit: &mut impl FnMut(Node<'ast, 'arena>),
) {
    visit(node);
    node.visit_children(|child| {
        if child.span().start.offset < cursor && !opens_variable_scope(child) {
            walk_file_scope_before_cursor(child, cursor, visit);
        }
    });
}

/// Whether a node's body runs with a fresh set of local variables rather than
/// the ones its surroundings hold.
fn opens_variable_scope(node: Node<'_, '_>) -> bool {
    matches!(
        node,
        Node::Function(_)
            | Node::Closure(_)
            | Node::ArrowFunction(_)
            | Node::Method(_)
            | Node::PropertyHook(_)
    )
}

/// Whether a construct ending at `end` is a better candidate than the one
/// already in `best`: it has to finish before the cursor, and later beats
/// earlier so the nearest preceding construct wins.
pub(in crate::virtual_members::laravel) fn beats_best<T>(
    best: &Option<(u32, T)>,
    end: u32,
    cursor: u32,
) -> bool {
    end <= cursor && best.as_ref().is_none_or(|(seen, _)| end >= *seen)
}

/// Walk statements, returning `Break` as soon as the visitor signals early exit.
fn walk_stmt_exprs(
    stmt: &Statement<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match stmt {
        Statement::Expression(e) => walk_expr_depth(e.expression, f)?,
        Statement::Return(r) => {
            if let Some(v) = r.value {
                walk_expr_depth(v, f)?;
            }
        }
        Statement::Echo(e) => {
            for v in e.values.iter() {
                walk_expr_depth(v, f)?;
            }
        }
        Statement::EchoTag(e) => {
            for v in e.values.iter() {
                walk_expr_depth(v, f)?;
            }
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Block(b) => {
            for s in b.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::If(if_stmt) => {
            walk_expr_depth(if_stmt.condition, f)?;
            crate::parser::try_for_each_if_branch(if_stmt, |statements| {
                for s in statements {
                    walk_stmt_exprs(s, f)?;
                }
                ControlFlow::Continue(())
            })?;
        }
        Statement::While(w) => {
            walk_expr_depth(w.condition, f)?;
            for s in w.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::DoWhile(dw) => {
            walk_expr_depth(dw.condition, f)?;
            walk_stmt_exprs(dw.statement, f)?;
        }
        Statement::For(fs) => {
            for init in fs.initializations.iter() {
                walk_expr_depth(init, f)?;
            }
            for cond in fs.conditions.iter() {
                walk_expr_depth(cond, f)?;
            }
            for update in fs.increments.iter() {
                walk_expr_depth(update, f)?;
            }
            for s in fs.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Foreach(fe) => {
            walk_expr_depth(fe.expression, f)?;
            for s in fe.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Try(t) => {
            for s in t.block.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
            for catch in t.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
            if let Some(ref fin) = t.finally_clause {
                for s in fin.block.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        Statement::Switch(sw) => {
            walk_expr_depth(sw.expression, f)?;
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(c) => {
                        walk_expr_depth(c.expression, f)?;
                        for s in c.statements.iter() {
                            walk_stmt_exprs(s, f)?;
                        }
                    }
                    SwitchCase::Default(c) => {
                        for s in c.statements.iter() {
                            walk_stmt_exprs(s, f)?;
                        }
                    }
                }
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        _ if let Some(body) = crate::parser::class_like_body(stmt) => {
            for member in body.members {
                walk_class_member_exprs(member, f)?;
            }
        }

        Statement::Static(s) => {
            for item in s.items.iter() {
                if let Some(init) = item.value() {
                    walk_expr_depth(init, f)?;
                }
            }
        }
        Statement::Unset(u) => {
            for v in u.values.iter() {
                walk_expr_depth(v, f)?;
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_class_member_exprs(
    member: &ClassLikeMember<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match member {
        ClassLikeMember::Method(method) => {
            if let MethodBody::Concrete(body) = &method.body {
                for s in body.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        ClassLikeMember::Property(Property::Plain(prop)) => {
            for item in prop.items.iter() {
                if let PropertyItem::Concrete(concrete) = item {
                    walk_expr_depth(concrete.value, f)?;
                }
            }
        }
        ClassLikeMember::Constant(c) => {
            for item in c.items.iter() {
                walk_expr_depth(item.value, f)?;
            }
        }
        ClassLikeMember::EnumCase(ec) => {
            if let EnumCaseItem::Backed(backed) = &ec.item {
                walk_expr_depth(backed.value, f)?;
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_expr_depth(
    expr: &Expression<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    f(expr)?;
    match expr {
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                walk_expr_depth(fc.function, f)?;
                for arg in fc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::StaticMethod(sc) => {
                for arg in sc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::Method(mc) => {
                walk_expr_depth(mc.object, f)?;
                for arg in mc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::NullSafeMethod(mc) => {
                walk_expr_depth(mc.object, f)?;
                for arg in mc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
        },
        Expression::Binary(b) => {
            walk_expr_depth(b.lhs, f)?;
            walk_expr_depth(b.rhs, f)?;
        }
        Expression::UnaryPrefix(u) => walk_expr_depth(u.operand, f)?,
        Expression::UnaryPostfix(u) => walk_expr_depth(u.operand, f)?,
        Expression::Parenthesized(p) => walk_expr_depth(p.expression, f)?,
        Expression::Assignment(a) => {
            walk_expr_depth(a.lhs, f)?;
            walk_expr_depth(a.rhs, f)?;
        }
        Expression::Conditional(c) => {
            walk_expr_depth(c.condition, f)?;
            if let Some(then) = c.then {
                walk_expr_depth(then, f)?;
            }
            walk_expr_depth(c.r#else, f)?;
        }
        Expression::Array(arr) => {
            for el in arr.elements.iter() {
                walk_array_el_depth(el, f)?;
            }
        }
        Expression::LegacyArray(arr) => {
            for el in arr.elements.iter() {
                walk_array_el_depth(el, f)?;
            }
        }
        Expression::ArrayAccess(a) => {
            walk_expr_depth(a.array, f)?;
            walk_expr_depth(a.index, f)?;
        }
        Expression::Closure(c) => {
            for s in c.body.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Expression::ArrowFunction(af) => walk_expr_depth(af.expression, f)?,
        Expression::Match(m) => {
            walk_expr_depth(m.expression, f)?;
            for arm in m.arms.iter() {
                match arm {
                    MatchArm::Expression(ea) => {
                        for cond in ea.conditions.iter() {
                            walk_expr_depth(cond, f)?;
                        }
                        walk_expr_depth(ea.expression, f)?;
                    }
                    MatchArm::Default(da) => walk_expr_depth(da.expression, f)?,
                }
            }
        }
        Expression::Throw(t) => walk_expr_depth(t.exception, f)?,
        Expression::Yield(y) => match y {
            Yield::Value(yv) => {
                if let Some(val) = yv.value {
                    walk_expr_depth(val, f)?;
                }
            }
            Yield::Pair(yp) => {
                walk_expr_depth(yp.key, f)?;
                walk_expr_depth(yp.value, f)?;
            }
            Yield::From(yf) => walk_expr_depth(yf.iterator, f)?,
        },
        Expression::Clone(c) => walk_expr_depth(c.object, f)?,
        Expression::Instantiation(inst) => {
            if let Some(args) = &inst.argument_list {
                for a in args.arguments.iter() {
                    walk_expr_depth(a.value(), f)?;
                }
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_array_el_depth(
    el: &ArrayElement<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match el {
        ArrayElement::KeyValue(kv) => {
            walk_expr_depth(kv.key, f)?;
            walk_expr_depth(kv.value, f)?;
        }
        ArrayElement::Value(v) => walk_expr_depth(v.value, f)?,
        ArrayElement::Variadic(v) => walk_expr_depth(v.value, f)?,
        ArrayElement::Missing(_) => {}
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
#[path = "expr_walker_tests.rs"]
mod tests;
