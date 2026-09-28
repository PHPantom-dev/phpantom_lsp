//! Enclosing function-like and property lookup for magic constants.
//!
//! `__FUNCTION__` and `__METHOD__` need the name of the function, method,
//! closure, or arrow function that *directly* encloses the magic constant,
//! not the outer method the forward walker started from: a nested closure
//! has its own name (`{closure}`) independent of the method it sits in.
//! `__PROPERTY__` similarly needs the name of the hooked property whose
//! `get`/`set` hook directly contains it. A dedicated AST walker finds
//! both in a single pass over the enclosing constructs.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::method::Method;
use mago_syntax::cst::class_like::property::HookedProperty;
use mago_syntax::cst::class_like::property::PropertyHook;
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

/// The function-like and property context lexically enclosing a magic
/// constant, captured together since both are found by the same walk.
#[derive(Default)]
pub(super) struct EnclosingContext {
    pub function: Option<EnclosingFunction>,
    /// The bare name (no `$`) of the innermost hooked property whose
    /// `get`/`set` hook contains the magic constant, if any.
    pub property: Option<String>,
}

struct FinderState {
    function_stack: Vec<EnclosingFunction>,
    property_stack: Vec<String>,
    target: u32,
    result: Option<EnclosingContext>,
}

struct EnclosingFunctionFinder;

impl<'ast, 'arena> Walker<'ast, 'arena, FinderState> for EnclosingFunctionFinder {
    fn walk_in_function(&self, node: &'ast Function<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.push(EnclosingFunction::Named(
            bytes_to_str(node.name.value).to_string(),
        ));
    }

    fn walk_out_function(&self, _node: &'ast Function<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.pop();
    }

    fn walk_in_method(&self, node: &'ast Method<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.push(EnclosingFunction::Named(
            bytes_to_str(node.name.value).to_string(),
        ));
    }

    fn walk_out_method(&self, _node: &'ast Method<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.pop();
    }

    fn walk_in_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.push(EnclosingFunction::Closure);
    }

    fn walk_out_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.pop();
    }

    fn walk_in_arrow_function(&self, _node: &'ast ArrowFunction<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.push(EnclosingFunction::Closure);
    }

    fn walk_out_arrow_function(&self, _node: &'ast ArrowFunction<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.pop();
    }

    fn walk_in_property_hook(&self, node: &'ast PropertyHook<'arena>, ctx: &mut FinderState) {
        let property_name = ctx.property_stack.last().map_or("", String::as_str);
        let hook_name = bytes_to_str(node.name.value);
        ctx.function_stack.push(EnclosingFunction::Named(format!(
            "${property_name}::{hook_name}"
        )));
    }

    fn walk_out_property_hook(&self, _node: &'ast PropertyHook<'arena>, ctx: &mut FinderState) {
        ctx.function_stack.pop();
    }

    fn walk_in_hooked_property(&self, node: &'ast HookedProperty<'arena>, ctx: &mut FinderState) {
        ctx.property_stack.push(
            bytes_to_str(node.item.variable().name)
                .trim_start_matches('$')
                .to_string(),
        );
    }

    fn walk_out_hooked_property(&self, _node: &'ast HookedProperty<'arena>, ctx: &mut FinderState) {
        ctx.property_stack.pop();
    }

    fn walk_in_magic_constant(&self, node: &'ast MagicConstant<'arena>, ctx: &mut FinderState) {
        if node.span().start.offset == ctx.target {
            ctx.result = Some(EnclosingContext {
                function: ctx.function_stack.last().cloned(),
                property: ctx.property_stack.last().cloned(),
            });
        }
    }
}

/// Find the function-like construct and hooked property whose bodies
/// directly contain the magic constant at `offset`.
///
/// Both fields are `None` for a magic constant written outside any
/// function-like construct or property hook, respectively.
pub(super) fn enclosing_context_at(content: &str, offset: u32) -> EnclosingContext {
    with_parsed_program(content, "magic_constant_enclosing_context", |program, _| {
        let mut state = FinderState {
            function_stack: Vec::new(),
            property_stack: Vec::new(),
            target: offset,
            result: None,
        };
        let walker = EnclosingFunctionFinder;
        for statement in program.statements.iter() {
            Walker::walk_statement(&walker, statement, &mut state);
        }
        state.result.unwrap_or_default()
    })
}
