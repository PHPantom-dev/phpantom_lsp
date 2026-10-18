//! What the enclosing function writes to a closure's `use (&$x)` captures
//! after the closure is created.
//!
//! A by-value capture is copied when the closure is created, so the scope
//! at that point is all it ever sees.  A by-reference capture is read when
//! the closure runs, which can be after the enclosing function has written
//! the variable again:
//!
//! ```php
//! if ($c !== null) {
//!     $f = function () use (&$c) { take($c); };   // `$c` may be null here
//!     $c = null;
//!     $f();
//! }
//! ```

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::walker::Walker;

use super::*;
use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;

/// Widen each `use (&$x)` capture of `closure` in `closure_scope` by the
/// types the code that can run after the closure is created writes to
/// `$x`: what follows it in the enclosing function, and the whole body of
/// any loop the closure is written in.
///
/// A closure that provably runs where it is written (called on the spot,
/// or handed to a callee that invokes it before returning) is left alone,
/// since nothing after it can run first.  Each write is evaluated against
/// `outer`, the scope the closure is created in.
pub(crate) fn widen_by_ref_captures_by_later_writes(
    closure: &Closure<'_>,
    closure_scope: &mut ScopeState,
    outer: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let captured = by_ref_captured_names(closure);
    if captured.is_empty() {
        return;
    }
    let span = closure.span();
    let target = (span.start.offset, span.end.offset);

    let widened = with_parsed_program(ctx.content, "by_ref_capture_later_writes", |program, _| {
        let mut site = Site {
            target,
            body: None,
            found: false,
            in_class_like: false,
            runs_in_place: false,
            outer,
            ctx,
        };
        SiteFinder.walk_program(program, &mut site);
        if !site.found || site.runs_in_place {
            return Vec::new();
        }

        let mut writes = Writes {
            target,
            names: &captured,
            enclosing_loops: 0,
            found: Vec::new(),
        };
        match site.body {
            Some(Body::Block(block)) => {
                for stmt in block.statements.iter() {
                    WriteCollector.walk_statement(stmt, &mut writes);
                }
            }
            Some(Body::Expression(expr)) => WriteCollector.walk_expression(expr, &mut writes),
            None if site.in_class_like => return Vec::new(),
            None => {
                for stmt in program.statements.iter() {
                    WriteCollector.walk_statement(stmt, &mut writes);
                }
            }
        }
        if writes.found.is_empty() {
            return Vec::new();
        }

        let probe_ctx = ctx.with_cursor_offset(u32::MAX);
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        let mut widened: Vec<(String, Option<Vec<ResolvedType>>)> = Vec::new();
        for (expr, written) in writes.found {
            let mut probe = outer.clone();
            process_assignment_expr(expr, &mut probe, &probe_ctx);
            for name in written {
                let types = probe.get(name);
                // A call that takes the variable by value, or a write
                // through one of its properties, leaves it as it was.
                if probe.contains(name) == outer.contains(name)
                    && !resolved_types_differ(types, outer.get(name))
                {
                    continue;
                }
                let entry = match widened.iter_mut().find(|(n, _)| n == name) {
                    Some((_, entry)) => entry,
                    None => {
                        widened.push((name.to_string(), Some(Vec::new())));
                        &mut widened.last_mut().expect("just pushed").1
                    }
                };
                // A write whose value cannot be resolved leaves the
                // capture unknown rather than keeping a narrowing it may
                // have undone.
                match entry {
                    Some(acc) if !types.is_empty() => {
                        ResolvedType::extend_unique(acc, types.to_vec());
                    }
                    _ => *entry = None,
                }
            }
        }
        widened
    });

    for (name, types) in widened {
        closure_scope.invalidate_dependent_keys(&name);
        closure_scope.invalidate_proofs(&name);
        match types {
            Some(types) => {
                let mut combined = closure_scope.get(&name).to_vec();
                ResolvedType::extend_unique(&mut combined, types);
                closure_scope.set(&name, combined);
            }
            None => closure_scope.set_empty(&name),
        }
    }
}

/// The function-like body a closure is written in.
enum Body<'ast, 'arena> {
    Block(&'ast Block<'arena>),
    /// An arrow function's body.
    Expression(&'ast Expression<'arena>),
}

/// What [`SiteFinder`] learns about where the closure spanning `target` is
/// written.
struct Site<'ast, 'arena, 's> {
    target: (u32, u32),
    /// The innermost function-like body holding the closure, or `None`
    /// for top-level code.
    body: Option<Body<'ast, 'arena>>,
    found: bool,
    /// Whether the closure sits in a class-like outside any method body
    /// (a property hook, say), where top-level code is not what runs
    /// after it.
    in_class_like: bool,
    runs_in_place: bool,
    outer: &'s ScopeState,
    ctx: &'s ForwardWalkCtx<'s>,
}

impl Site<'_, '_, '_> {
    fn holds_target(&self, span: mago_span::Span) -> bool {
        span.start.offset <= self.target.0
            && self.target.1 <= span.end.offset
            && (span.start.offset, span.end.offset) != self.target
    }

    fn is_target(&self, expr: &Expression<'_>) -> bool {
        let span = crate::parser::unwrap_parens(expr).span();
        matches!(crate::parser::unwrap_parens(expr), Expression::Closure(_))
            && (span.start.offset, span.end.offset) == self.target
    }
}

struct SiteFinder;

impl<'ast, 'arena, 's> Walker<'ast, 'arena, Site<'ast, 'arena, 's>> for SiteFinder {
    fn walk_in_function(
        &self,
        function: &'ast Function<'arena>,
        site: &mut Site<'ast, 'arena, 's>,
    ) {
        if site.holds_target(function.span()) {
            site.body = Some(Body::Block(&function.body));
        }
    }

    fn walk_in_method(&self, method: &'ast Method<'arena>, site: &mut Site<'ast, 'arena, 's>) {
        if site.holds_target(method.span())
            && let MethodBody::Concrete(block) = &method.body
        {
            site.body = Some(Body::Block(block));
        }
    }

    fn walk_in_closure(&self, closure: &'ast Closure<'arena>, site: &mut Site<'ast, 'arena, 's>) {
        let span = closure.span();
        if (span.start.offset, span.end.offset) == site.target {
            site.found = true;
        } else if site.holds_target(span) {
            site.body = Some(Body::Block(&closure.body));
        }
    }

    fn walk_in_arrow_function(
        &self,
        arrow: &'ast ArrowFunction<'arena>,
        site: &mut Site<'ast, 'arena, 's>,
    ) {
        if site.holds_target(arrow.span()) {
            site.body = Some(Body::Expression(arrow.expression));
        }
    }

    fn walk_in_class(&self, class: &'ast Class<'arena>, site: &mut Site<'ast, 'arena, 's>) {
        site.in_class_like |= site.holds_target(class.span());
    }

    fn walk_in_trait(&self, r#trait: &'ast Trait<'arena>, site: &mut Site<'ast, 'arena, 's>) {
        site.in_class_like |= site.holds_target(r#trait.span());
    }

    fn walk_in_enum(&self, r#enum: &'ast Enum<'arena>, site: &mut Site<'ast, 'arena, 's>) {
        site.in_class_like |= site.holds_target(r#enum.span());
    }

    fn walk_in_anonymous_class(
        &self,
        class: &'ast AnonymousClass<'arena>,
        site: &mut Site<'ast, 'arena, 's>,
    ) {
        site.in_class_like |= site.holds_target(class.span());
    }

    fn walk_in_expression(
        &self,
        expr: &'ast Expression<'arena>,
        site: &mut Site<'ast, 'arena, 's>,
    ) {
        let Expression::Call(call) = expr else {
            return;
        };
        if let Call::Function(fc) = call
            && site.is_target(fc.function)
        {
            site.runs_in_place = true;
            return;
        }
        let args = match call {
            Call::Function(fc) => &fc.argument_list,
            Call::Method(mc) => &mc.argument_list,
            Call::NullSafeMethod(mc) => &mc.argument_list,
            Call::StaticMethod(sc) => &sc.argument_list,
        };
        let mut next_positional = 0usize;
        for arg in args.arguments.iter() {
            let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
            if site.is_target(arg_expr) {
                site.runs_in_place =
                    call_invokes_arg_immediately(call, &selector, site.outer, site.ctx);
                return;
            }
        }
    }
}

/// The writes [`WriteCollector`] finds to the captured `names`, each with
/// the names it writes.
struct Writes<'ast, 'arena, 'n> {
    target: (u32, u32),
    names: &'n [String],
    /// How many of the loops being walked hold the closure.  Inside one,
    /// every write can run after the closure is created, by way of the
    /// next iteration.
    enclosing_loops: u32,
    found: Vec<(&'ast Expression<'arena>, Vec<&'n str>)>,
}

impl<'n> Writes<'_, '_, 'n> {
    fn holds_target(&self, span: mago_span::Span) -> bool {
        span.start.offset <= self.target.0 && self.target.1 <= span.end.offset
    }

    fn captured(&self, vars: &HashSet<String>) -> Vec<&'n str> {
        self.names
            .iter()
            .filter(|name| vars.contains(name.as_str()))
            .map(String::as_str)
            .collect()
    }
}

/// Collects the writes to the captured variables in one function-like
/// body, leaving out nested declarations and closures: they have scopes
/// of their own.
struct WriteCollector;

macro_rules! count_enclosing_loop {
    ($walk_in:ident, $walk_out:ident, $node:ty) => {
        fn $walk_in(&self, node: &'ast $node, writes: &mut Writes<'ast, 'arena, 'n>) {
            if writes.holds_target(node.span()) {
                writes.enclosing_loops += 1;
            }
        }

        fn $walk_out(&self, node: &'ast $node, writes: &mut Writes<'ast, 'arena, 'n>) {
            if writes.holds_target(node.span()) {
                writes.enclosing_loops -= 1;
            }
        }
    };
}

impl<'ast, 'arena, 'n> Walker<'ast, 'arena, Writes<'ast, 'arena, 'n>> for WriteCollector {
    fn walk_closure(&self, _: &'ast Closure<'arena>, _: &mut Writes<'ast, 'arena, 'n>) {}
    fn walk_arrow_function(
        &self,
        _: &'ast ArrowFunction<'arena>,
        _: &mut Writes<'ast, 'arena, 'n>,
    ) {
    }
    fn walk_function(&self, _: &'ast Function<'arena>, _: &mut Writes<'ast, 'arena, 'n>) {}
    fn walk_class(&self, _: &'ast Class<'arena>, _: &mut Writes<'ast, 'arena, 'n>) {}
    fn walk_interface(&self, _: &'ast Interface<'arena>, _: &mut Writes<'ast, 'arena, 'n>) {}
    fn walk_trait(&self, _: &'ast Trait<'arena>, _: &mut Writes<'ast, 'arena, 'n>) {}
    fn walk_enum(&self, _: &'ast Enum<'arena>, _: &mut Writes<'ast, 'arena, 'n>) {}
    fn walk_anonymous_class(
        &self,
        _: &'ast AnonymousClass<'arena>,
        _: &mut Writes<'ast, 'arena, 'n>,
    ) {
    }

    count_enclosing_loop!(walk_in_foreach, walk_out_foreach, Foreach<'arena>);
    count_enclosing_loop!(walk_in_for, walk_out_for, For<'arena>);
    count_enclosing_loop!(walk_in_while, walk_out_while, While<'arena>);
    count_enclosing_loop!(walk_in_do_while, walk_out_do_while, DoWhile<'arena>);

    fn walk_in_expression(
        &self,
        expr: &'ast Expression<'arena>,
        writes: &mut Writes<'ast, 'arena, 'n>,
    ) {
        if writes.enclosing_loops == 0 && expr.span().start.offset < writes.target.1 {
            return;
        }
        let mut vars = HashSet::new();
        if let Some(operand) = increment_decrement_operand(expr) {
            collect_assignment_target_vars(operand, &mut vars);
        } else {
            match expr {
                Expression::Assignment(assignment) => {
                    collect_assignment_target_vars(assignment.lhs, &mut vars);
                }
                // A variable passed to a by-reference parameter is written
                // by the call.
                Expression::Call(call) => {
                    let args = match call {
                        Call::Function(fc) => &fc.argument_list,
                        Call::Method(mc) => &mc.argument_list,
                        Call::NullSafeMethod(mc) => &mc.argument_list,
                        Call::StaticMethod(sc) => &sc.argument_list,
                    };
                    for arg in args.arguments.iter() {
                        if let Expression::Variable(Variable::Direct(var)) = arg.value() {
                            vars.insert(bytes_to_str(var.name).to_string());
                        }
                    }
                }
                _ => return,
            }
        }
        let written = writes.captured(&vars);
        if !written.is_empty() {
            writes.found.push((expr, written));
        }
    }
}
