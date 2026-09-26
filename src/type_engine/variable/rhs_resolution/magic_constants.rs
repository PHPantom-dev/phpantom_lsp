//! Enclosing function-like lookup for magic constants.
//!
//! `__FUNCTION__` and `__METHOD__` need the name of the function, method,
//! closure, or arrow function that *directly* encloses the magic constant,
//! not the outer method the forward walker started from: a nested closure
//! has its own name (`{closure}`) independent of the method it sits in.
//! A dedicated AST walker finds that innermost function-like construct.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::method::Method;
use mago_syntax::cst::function_like::arrow_function::ArrowFunction;
use mago_syntax::cst::function_like::closure::Closure;
use mago_syntax::cst::function_like::function::Function;
use mago_syntax::cst::magic_constant::MagicConstant;
use mago_syntax::walker::Walker;

use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;

/// The function-like construct lexically enclosing a magic constant.
#[derive(Clone)]
pub(super) enum EnclosingFunction {
    /// A named top-level function or class method, carrying its bare name
    /// (`doFoo`, not `App\User::doFoo`).
    Named(String),
    /// A closure or arrow function, which PHP always names `{closure}`.
    Closure,
}

struct FinderState {
    stack: Vec<EnclosingFunction>,
    target: u32,
    result: Option<EnclosingFunction>,
}

struct EnclosingFunctionFinder;

impl<'ast, 'arena> Walker<'ast, 'arena, FinderState> for EnclosingFunctionFinder {
    fn walk_in_function(&self, node: &'ast Function<'arena>, ctx: &mut FinderState) {
        ctx.stack.push(EnclosingFunction::Named(
            bytes_to_str(node.name.value).to_string(),
        ));
    }

    fn walk_out_function(&self, _node: &'ast Function<'arena>, ctx: &mut FinderState) {
        ctx.stack.pop();
    }

    fn walk_in_method(&self, node: &'ast Method<'arena>, ctx: &mut FinderState) {
        ctx.stack.push(EnclosingFunction::Named(
            bytes_to_str(node.name.value).to_string(),
        ));
    }

    fn walk_out_method(&self, _node: &'ast Method<'arena>, ctx: &mut FinderState) {
        ctx.stack.pop();
    }

    fn walk_in_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut FinderState) {
        ctx.stack.push(EnclosingFunction::Closure);
    }

    fn walk_out_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut FinderState) {
        ctx.stack.pop();
    }

    fn walk_in_arrow_function(&self, _node: &'ast ArrowFunction<'arena>, ctx: &mut FinderState) {
        ctx.stack.push(EnclosingFunction::Closure);
    }

    fn walk_out_arrow_function(&self, _node: &'ast ArrowFunction<'arena>, ctx: &mut FinderState) {
        ctx.stack.pop();
    }

    fn walk_in_magic_constant(&self, node: &'ast MagicConstant<'arena>, ctx: &mut FinderState) {
        if node.span().start.offset == ctx.target {
            ctx.result = ctx.stack.last().cloned();
        }
    }
}

/// Find the function, method, closure, or arrow function whose body
/// directly contains the magic constant at `offset`.
///
/// Returns `None` for a magic constant written in top-level code, outside
/// any function-like construct.
pub(super) fn enclosing_function_at(content: &str, offset: u32) -> Option<EnclosingFunction> {
    with_parsed_program(
        content,
        "magic_constant_enclosing_function",
        |program, _| {
            let mut state = FinderState {
                stack: Vec::new(),
                target: offset,
                result: None,
            };
            let walker = EnclosingFunctionFinder;
            for statement in program.statements.iter() {
                Walker::walk_statement(&walker, statement, &mut state);
            }
            state.result
        },
    )
}
