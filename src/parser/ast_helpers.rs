//! Small helpers for walking the mago-syntax AST: seeing through
//! parentheses and negation, recognising `X::class`, reading array
//! literals, and visiting class members, `if` branches, nested
//! statement lists and the yields a function body owns.

/// Whether `access` fetches a class name (`X::class`) rather than a named
/// constant.  `class` is a keyword, so any casing counts.
pub(crate) fn is_class_fetch(access: &mago_syntax::cst::access::ClassConstantAccess<'_>) -> bool {
    matches!(
        &access.constant,
        mago_syntax::cst::class_like::member::ClassLikeConstantSelector::Identifier(constant)
            if constant.value.eq_ignore_ascii_case(b"class")
    )
}

/// The class expression `X` of an `X::class` fetch, or `None` for any
/// other expression.
///
/// `self`, `static` and `parent` parse as expression kinds of their own, so
/// a caller that only wants a class name as written matches
/// `Expression::Identifier` on the result (see [`class_fetch_identifier`]).
pub(crate) fn class_fetch_target<'ast>(
    expr: &mago_syntax::cst::Expression<'ast>,
) -> Option<&'ast mago_syntax::cst::Expression<'ast>> {
    use mago_syntax::cst::{Access, Expression};
    match expr {
        Expression::Access(Access::ClassConstant(access)) if is_class_fetch(access) => {
            Some(access.class)
        }
        _ => None,
    }
}

/// The class name written before `::class` (`Foo` in `Foo::class`), or
/// `None` for `self::class`, `static::class`, `parent::class`, `$x::class`
/// and any other expression.
pub(crate) fn class_fetch_identifier<'ast>(
    expr: &mago_syntax::cst::Expression<'ast>,
) -> Option<&'ast mago_syntax::cst::Identifier<'ast>> {
    match class_fetch_target(expr)? {
        mago_syntax::cst::Expression::Identifier(ident) => Some(ident),
        _ => None,
    }
}

/// Unwrap parenthesized expressions to their inner expression.
pub(crate) fn unwrap_parens<'a>(
    expr: &'a mago_syntax::cst::Expression<'a>,
) -> &'a mago_syntax::cst::Expression<'a> {
    match expr {
        mago_syntax::cst::Expression::Parenthesized(p) => unwrap_parens(p.expression),
        other => other,
    }
}

/// Unwrap parentheses and any number of `!` prefixes from a condition,
/// returning the expression underneath and whether an odd number of `!`s
/// wrapped it.
pub(crate) fn unwrap_negation<'a>(
    expr: &'a mago_syntax::cst::Expression<'a>,
) -> (&'a mago_syntax::cst::Expression<'a>, bool) {
    use mago_syntax::cst::Expression;
    let mut negated = false;
    let mut inner = expr;
    loop {
        match inner {
            Expression::Parenthesized(p) => inner = p.expression,
            Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
                negated = !negated;
                inner = prefix.operand;
            }
            _ => return (inner, negated),
        }
    }
}

/// The elements of an array literal, in either spelling (`[…]` and the
/// legacy `array(…)`).
///
/// `None` for anything that is not an array literal.
pub(crate) fn array_literal_elements<'a, 'ast>(
    expr: &'a mago_syntax::cst::Expression<'ast>,
) -> Option<
    &'a mago_syntax::cst::sequence::TokenSeparatedSequence<
        'ast,
        mago_syntax::cst::array::ArrayElement<'ast>,
    >,
> {
    use mago_syntax::cst::Expression;
    match expr {
        Expression::Array(array) => Some(&array.elements),
        Expression::LegacyArray(array) => Some(&array.elements),
        _ => None,
    }
}

/// The value expression of an array element, or `None` for a hole in a
/// list (`[$a, , $c]`).
pub(crate) fn array_element_value<'b>(
    elem: &'b mago_syntax::cst::array::ArrayElement<'b>,
) -> Option<&'b mago_syntax::cst::Expression<'b>> {
    use mago_syntax::cst::array::ArrayElement;
    match elem {
        ArrayElement::KeyValue(kv) => Some(kv.value),
        ArrayElement::Value(v) => Some(v.value),
        ArrayElement::Variadic(v) => Some(v.value),
        ArrayElement::Missing(_) => None,
    }
}

/// The key expression of an array element written as `key => value`.
pub(crate) fn array_element_key<'b>(
    elem: &'b mago_syntax::cst::array::ArrayElement<'b>,
) -> Option<&'b mago_syntax::cst::Expression<'b>> {
    use mago_syntax::cst::array::ArrayElement;
    match elem {
        mago_syntax::cst::array::ArrayElement::KeyValue(kv) => Some(kv.key),
        ArrayElement::Value(_) | ArrayElement::Variadic(_) | ArrayElement::Missing(_) => None,
    }
}

/// The member list of a class-like node, in any of the four spellings.
///
/// `None` for an interface (whose methods have no body) and for every
/// other node kind, which a walker treats as "nothing to descend into
/// here".
pub(crate) fn class_like_members<'ast>(
    node: mago_syntax::cst::node::Node<'ast, 'ast>,
) -> Option<&'ast [mago_syntax::cst::class_like::member::ClassLikeMember<'ast>]> {
    use mago_syntax::cst::node::Node;
    match node {
        Node::Class(class) => Some(class.members.as_slice()),
        Node::AnonymousClass(class) => Some(class.members.as_slice()),
        Node::Trait(trait_) => Some(trait_.members.as_slice()),
        Node::Enum(enum_) => Some(enum_.members.as_slice()),
        _ => None,
    }
}

/// The `{ … }` body of a class-like declaration statement.
#[derive(Clone, Copy)]
pub(crate) struct ClassLikeBody<'s, 'ast> {
    pub left_brace: mago_span::Span,
    pub members: &'s [mago_syntax::cst::class_like::member::ClassLikeMember<'ast>],
    pub right_brace: mago_span::Span,
}

impl ClassLikeBody<'_, '_> {
    /// Whether `offset` falls between the braces, both included.
    pub(crate) fn contains(&self, offset: u32) -> bool {
        self.left_brace.start.offset <= offset && offset <= self.right_brace.end.offset
    }
}

/// The body of a class, interface, trait or enum declaration, or `None` for
/// any other statement.
pub(crate) fn class_like_body<'s, 'ast>(
    stmt: &'s mago_syntax::cst::Statement<'ast>,
) -> Option<ClassLikeBody<'s, 'ast>> {
    use mago_syntax::cst::Statement;
    let (left_brace, members, right_brace) = match stmt {
        Statement::Class(class) => (class.left_brace, &class.members, class.right_brace),
        Statement::Interface(iface) => (iface.left_brace, &iface.members, iface.right_brace),
        Statement::Trait(trait_) => (trait_.left_brace, &trait_.members, trait_.right_brace),
        Statement::Enum(enum_) => (enum_.left_brace, &enum_.members, enum_.right_brace),
        _ => return None,
    };
    Some(ClassLikeBody {
        left_brace,
        members: members.as_slice(),
        right_brace,
    })
}

/// Call `visit` for each method of `members` that has a body, handing it
/// the method and the block.
pub(crate) fn for_each_concrete_method<'ast>(
    members: &'ast [mago_syntax::cst::class_like::member::ClassLikeMember<'ast>],
    mut visit: impl FnMut(
        &'ast mago_syntax::cst::class_like::method::Method<'ast>,
        &'ast mago_syntax::cst::Block<'ast>,
    ),
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;
    for member in members {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(body) = &method.body
        {
            visit(method, body);
        }
    }
}

/// Call `visit` with each statement list an `if` can run: the body, the
/// body of every `elseif`, and the `else`.
///
/// Both spellings of an `if` (braced and `:`-delimited) are covered, since
/// `IfBody` already unifies them.
pub(crate) fn for_each_if_branch(
    if_stmt: &mago_syntax::cst::If<'_>,
    mut visit: impl FnMut(&[mago_syntax::cst::Statement<'_>]),
) {
    let _ = try_for_each_if_branch(if_stmt, |statements| {
        visit(statements);
        std::ops::ControlFlow::<()>::Continue(())
    });
}

/// [`for_each_if_branch`] for a walker that stops early: the first branch
/// `visit` breaks on ends the walk and its value comes back.
pub(crate) fn try_for_each_if_branch<B>(
    if_stmt: &mago_syntax::cst::If<'_>,
    mut visit: impl FnMut(&[mago_syntax::cst::Statement<'_>]) -> std::ops::ControlFlow<B>,
) -> std::ops::ControlFlow<B> {
    visit(if_stmt.body.statements())?;
    for statements in if_stmt.body.else_if_statements() {
        visit(statements)?;
    }
    if let Some(statements) = if_stmt.body.else_statements() {
        visit(statements)?;
    }
    std::ops::ControlFlow::Continue(())
}

/// Call `visit` with each statement list nested directly inside `stmt`
/// that runs in the enclosing scope: a block, a namespace or `declare`
/// body, every `if` branch, a loop body, each `switch` case, and the
/// `try`, `catch` and `finally` blocks.
///
/// Function, class and closure bodies open a scope of their own and are
/// not visited; a walker that wants them matches those arms itself.
pub(crate) fn for_each_nested_statement_list<'s, 'ast>(
    stmt: &'s mago_syntax::cst::Statement<'ast>,
    mut visit: impl FnMut(&'s [mago_syntax::cst::Statement<'ast>]),
) {
    let _ = try_for_each_nested_statement_list(stmt, |statements| {
        visit(statements);
        std::ops::ControlFlow::<()>::Continue(())
    });
}

/// Whether `pred` holds for any statement nested directly inside `stmt`,
/// in the sense of [`for_each_nested_statement_list`].
pub(crate) fn any_nested_statement<'s, 'ast>(
    stmt: &'s mago_syntax::cst::Statement<'ast>,
    mut pred: impl FnMut(&'s mago_syntax::cst::Statement<'ast>) -> bool,
) -> bool {
    find_map_nested_statement(stmt, |inner| pred(inner).then_some(())).is_some()
}

/// The first `Some` that `f` returns for a statement nested directly
/// inside `stmt`, in the sense of [`for_each_nested_statement_list`].
pub(crate) fn find_map_nested_statement<'s, 'ast, T>(
    stmt: &'s mago_syntax::cst::Statement<'ast>,
    mut f: impl FnMut(&'s mago_syntax::cst::Statement<'ast>) -> Option<T>,
) -> Option<T> {
    try_for_each_nested_statement_list(stmt, |statements| {
        match statements.iter().find_map(&mut f) {
            Some(found) => std::ops::ControlFlow::Break(found),
            None => std::ops::ControlFlow::Continue(()),
        }
    })
    .break_value()
}

/// [`for_each_nested_statement_list`] for a walker that stops early: the
/// first list `visit` breaks on ends the walk and its value comes back.
pub(crate) fn try_for_each_nested_statement_list<'s, 'ast, B>(
    stmt: &'s mago_syntax::cst::Statement<'ast>,
    mut visit: impl FnMut(&'s [mago_syntax::cst::Statement<'ast>]) -> std::ops::ControlFlow<B>,
) -> std::ops::ControlFlow<B> {
    use mago_syntax::cst::Statement;
    use mago_syntax::cst::control_flow::r#if::IfBody;
    use mago_syntax::cst::declare::DeclareBody;
    use std::slice::from_ref;
    match stmt {
        Statement::Block(block) => visit(block.statements.as_slice()),
        Statement::Namespace(ns) => visit(ns.statements().as_slice()),
        Statement::Declare(declare) => match &declare.body {
            DeclareBody::Statement(inner) => visit(from_ref(*inner)),
            DeclareBody::ColonDelimited(body) => visit(body.statements.as_slice()),
        },
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                visit(from_ref(body.statement))?;
                for clause in body.else_if_clauses.iter() {
                    visit(from_ref(clause.statement))?;
                }
                match &body.else_clause {
                    Some(clause) => visit(from_ref(clause.statement)),
                    None => std::ops::ControlFlow::Continue(()),
                }
            }
            IfBody::ColonDelimited(body) => {
                visit(body.statements.as_slice())?;
                for clause in body.else_if_clauses.iter() {
                    visit(clause.statements.as_slice())?;
                }
                match &body.else_clause {
                    Some(clause) => visit(clause.statements.as_slice()),
                    None => std::ops::ControlFlow::Continue(()),
                }
            }
        },
        Statement::While(w) => visit(w.body.statements()),
        Statement::DoWhile(dw) => visit(from_ref(dw.statement)),
        Statement::For(f) => visit(f.body.statements()),
        Statement::Foreach(f) => visit(f.body.statements()),
        Statement::Switch(switch) => {
            for case in switch.body.cases() {
                visit(case.statements())?;
            }
            std::ops::ControlFlow::Continue(())
        }
        Statement::Try(try_stmt) => {
            visit(try_stmt.block.statements.as_slice())?;
            for catch in try_stmt.catch_clauses.iter() {
                visit(catch.block.statements.as_slice())?;
            }
            match &try_stmt.finally_clause {
                Some(finally) => visit(finally.block.statements.as_slice()),
                None => std::ops::ControlFlow::Continue(()),
            }
        }
        _ => std::ops::ControlFlow::Continue(()),
    }
}

/// Call `visit` with each function declared inside `stmt`'s nested
/// statement lists, at any depth of blocks and control flow (the
/// `if (!function_exists('f')) { function f() {} }` polyfill pattern), but
/// not inside another function or class.
pub(crate) fn for_each_nested_function<'s, 'ast>(
    stmt: &'s mago_syntax::cst::Statement<'ast>,
    visit: &mut impl FnMut(&'s mago_syntax::cst::function_like::function::Function<'ast>),
) {
    for_each_nested_statement_list(stmt, |statements| {
        for inner in statements {
            match inner {
                mago_syntax::cst::Statement::Function(function) => visit(function),
                _ => for_each_nested_function(inner, visit),
            }
        }
    });
}

/// The walker behind [`for_each_own_yield`]: visits each `yield` without
/// entering the closures, arrow functions, functions and classes nested in
/// what it walks.
struct OwnYields;

impl<'ast, 'arena: 'ast, F: FnMut(&'ast mago_syntax::cst::Yield<'arena>)>
    mago_syntax::walker::Walker<'ast, 'arena, F> for OwnYields
{
    fn walk_closure(&self, _: &'ast mago_syntax::cst::Closure<'arena>, _: &mut F) {}
    fn walk_arrow_function(&self, _: &'ast mago_syntax::cst::ArrowFunction<'arena>, _: &mut F) {}
    fn walk_function(&self, _: &'ast mago_syntax::cst::Function<'arena>, _: &mut F) {}
    fn walk_anonymous_class(&self, _: &'ast mago_syntax::cst::AnonymousClass<'arena>, _: &mut F) {}
    fn walk_class(&self, _: &'ast mago_syntax::cst::Class<'arena>, _: &mut F) {}
    fn walk_interface(&self, _: &'ast mago_syntax::cst::Interface<'arena>, _: &mut F) {}
    fn walk_trait(&self, _: &'ast mago_syntax::cst::Trait<'arena>, _: &mut F) {}
    fn walk_enum(&self, _: &'ast mago_syntax::cst::Enum<'arena>, _: &mut F) {}

    fn walk_in_yield(&self, yield_expr: &'ast mago_syntax::cst::Yield<'arena>, visit: &mut F) {
        visit(yield_expr);
    }
}

/// Call `visit` with each `yield` the function body `statements` owns,
/// wherever it sits in an expression (`$sent = yield $value;`,
/// `log(yield)`).
///
/// The yields of the closures, arrow functions, functions and classes
/// nested in the body are skipped: they make *those* generators.
pub(crate) fn for_each_own_yield<'ast, 'arena: 'ast>(
    statements: impl IntoIterator<Item = &'ast mago_syntax::cst::Statement<'arena>>,
    mut visit: impl FnMut(&'ast mago_syntax::cst::Yield<'arena>),
) {
    use mago_syntax::walker::Walker;
    for stmt in statements {
        OwnYields.walk_statement(stmt, &mut visit);
    }
}

/// [`for_each_own_yield`] for an arrow function's body expression.
pub(crate) fn for_each_own_yield_in_expression<'ast, 'arena: 'ast>(
    expression: &'ast mago_syntax::cst::Expression<'arena>,
    mut visit: impl FnMut(&'ast mago_syntax::cst::Yield<'arena>),
) {
    use mago_syntax::walker::Walker;
    OwnYields.walk_expression(expression, &mut visit);
}

/// Whether the function body `statements` makes its function a generator:
/// it holds a `yield` of its own, in the sense of [`for_each_own_yield`].
pub(crate) fn contains_own_yield<'ast, 'arena: 'ast>(
    statements: impl IntoIterator<Item = &'ast mago_syntax::cst::Statement<'arena>>,
) -> bool {
    let mut found = false;
    for_each_own_yield(statements, |_| found = true);
    found
}

/// What `pick` makes of the innermost node inside `root` that holds
/// `offset` and that it accepts.
///
/// The walk only descends into the subtree holding `offset`, so it costs
/// the depth of the offset rather than the size of `root`.  Spans are
/// inclusive at both ends: an offset just past a construct is still on it.
/// It loops rather than recursing, so a deeply nested expression (a long
/// left-associative concatenation) cannot overflow the stack.
pub(crate) fn innermost_node_at<'ast, 'arena, T>(
    root: mago_syntax::cst::Node<'ast, 'arena>,
    offset: u32,
    mut pick: impl FnMut(mago_syntax::cst::Node<'ast, 'arena>) -> Option<T>,
) -> Option<T> {
    use mago_span::HasSpan;
    let holds = |node: &mago_syntax::cst::Node<'_, '_>| {
        let span = node.span();
        span.start.offset <= offset && offset <= span.end.offset
    };
    if !holds(&root) {
        return None;
    }
    let mut found = None;
    let mut node = root;
    loop {
        if let Some(picked) = pick(node) {
            found = Some(picked);
        }
        // An offset on the boundary of two siblings belongs to the later.
        let mut next = None;
        node.visit_children(|child| {
            if holds(&child) {
                next = Some(child);
            }
        });
        match next {
            Some(child) => node = child,
            None => return found,
        }
    }
}

/// Call `visit` with every node under `root`, in source order, together
/// with the nodes enclosing it (outermost first).
///
/// Walks with an explicit stack rather than recursing, so a deeply nested
/// expression cannot overflow the thread's stack.
pub(crate) fn for_each_node_with_ancestors<'ast, 'arena>(
    root: mago_syntax::cst::Node<'ast, 'arena>,
    mut visit: impl FnMut(mago_syntax::cst::Node<'ast, 'arena>, &[mago_syntax::cst::Node<'ast, 'arena>]),
) {
    let mut ancestors: Vec<mago_syntax::cst::Node<'ast, 'arena>> = Vec::new();
    // Each entry is a node and the number of ancestors it has.
    let mut pending = vec![(root, 0usize)];
    let mut children = Vec::new();
    while let Some((node, depth)) = pending.pop() {
        ancestors.truncate(depth);
        visit(node, &ancestors);
        ancestors.push(node);
        node.visit_children(|child| children.push(child));
        pending.extend(children.drain(..).rev().map(|child| (child, depth + 1)));
    }
}

/// One property a PHP 8.5 `clone($object, [...])` call writes on the copy:
/// a literal string key of its property array.
pub(crate) struct CloneWithWrite<'ast> {
    /// The property name, without the `$`.
    pub name: &'ast str,
    /// Byte range of the name inside the key's quotes.
    pub start: u32,
    pub end: u32,
    /// The value written to the property.
    pub value: &'ast mago_syntax::cst::Expression<'ast>,
}

/// The object a two-argument `clone($object, [...])` call copies, and the
/// properties its array literal writes on the copy.
///
/// Both arguments may be passed by name (`object:`, `withProperties:`).
/// Only keys spelled as a plain string literal whose source text is the
/// name itself are listed: a computed or escaped key names nothing a span
/// can point at.  `None` for any other call, and for a property array that
/// is not an array literal.
pub(crate) fn clone_with_writes<'ast>(
    call: &mago_syntax::cst::FunctionCall<'ast>,
    content: &str,
) -> Option<(
    &'ast mago_syntax::cst::Expression<'ast>,
    Vec<CloneWithWrite<'ast>>,
)> {
    use mago_syntax::cst::{Argument, Expression};

    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    if !ident.value().eq_ignore_ascii_case(b"clone") {
        return None;
    }

    let mut object = None;
    let mut properties = None;
    let mut position = 0usize;
    for arg in call.argument_list.arguments.iter() {
        match arg {
            Argument::Positional(pos) => {
                if pos.ellipsis.is_some() {
                    return None;
                }
                match position {
                    0 => object = Some(pos.value),
                    1 => properties = Some(pos.value),
                    _ => {}
                }
                position += 1;
            }
            Argument::Named(named) => match named.name.value {
                b"object" => object = Some(named.value),
                b"withProperties" => properties = Some(named.value),
                _ => {}
            },
        }
    }
    let elements = array_literal_elements(properties?)?;

    let writes = elements
        .iter()
        .filter_map(|element| {
            let key = array_element_key(element)?;
            let value = array_element_value(element)?;
            let (name, start, end) =
                crate::virtual_members::laravel::helpers::resolved_string_literal(key, content)?;
            (content.get(start..end) == Some(name)).then_some(CloneWithWrite {
                name,
                start: start as u32,
                end: end as u32,
                value,
            })
        })
        .collect();
    Some((object?, writes))
}
