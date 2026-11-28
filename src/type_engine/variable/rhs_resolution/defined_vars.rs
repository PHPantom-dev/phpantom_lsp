//! `get_defined_vars()`: a shape of the variables in scope where it is
//! called, keyed by name in the order PHP lists them.
//!
//! Also the lookup of the function-like a call is written in, which
//! `Closure::getCurrent()` shares.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::method::Method;
use mago_syntax::cst::function_like::function::Function;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, ShapeEntry};
use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

/// The variables PHP fills in itself, which `get_defined_vars()` leaves out
/// of a function's list.
const SUPERGLOBALS: &[&str] = &[
    "$this",
    "$GLOBALS",
    "$_SERVER",
    "$_GET",
    "$_POST",
    "$_FILES",
    "$_COOKIE",
    "$_SESSION",
    "$_REQUEST",
    "$_ENV",
];

/// `get_defined_vars()` inside a function-like: a shape of the variables in
/// scope, each optional when some path reaching the call never assigned it.
/// In the global scope any included file may have defined anything, so the
/// answer is `array<string, mixed>`.
///
/// `None` without a walker scope to list, which leaves the declared `array`.
pub(super) fn defined_vars_call(
    name: &str,
    call: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !name
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("get_defined_vars")
        || !argument_list.arguments.is_empty()
    {
        return None;
    }
    let offset = call.span().start.offset;
    let Some(start) = enclosing_function_start(ctx.file.content, offset) else {
        let any = PhpType::generic_array(PhpType::string(), PhpType::mixed());
        return Some(vec![ResolvedType::from_type_string(any)]);
    };
    let proofs = ctx.scope_proofs?;
    // PHP lists a function's variables in the order they first appear in
    // its source, parameters first.
    let body = ctx.file.content.get(start as usize..offset as usize)?;
    let mut entries: Vec<(usize, ShapeEntry)> = proofs
        .locals
        .variables()
        .filter(|(var, _)| !SUPERGLOBALS.contains(&var.as_str()))
        .map(|(var, types)| {
            let entry = ShapeEntry {
                key: Some(var.trim_start_matches('$').to_string()),
                optional: proofs.maybe_undefined.contains_key(var),
                value_type: if types.is_empty() {
                    PhpType::mixed()
                } else {
                    ResolvedType::types_joined(types)
                },
            };
            (first_occurrence(body, var).unwrap_or(usize::MAX), entry)
        })
        .collect();
    entries.sort_by_key(|(position, _)| *position);
    let shape = PhpType::array_shape(entries.into_iter().map(|(_, entry)| entry).collect());
    Some(vec![ResolvedType::from_type_string(shape)])
}

/// Where `var` (with its `$`) is first written in `body`, as a whole name.
fn first_occurrence(body: &str, var: &str) -> Option<usize> {
    body.match_indices(var).map(|(at, _)| at).find(|&at| {
        !body[at + var.len()..]
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80)
    })
}

/// The start of the innermost function, method, closure or arrow function
/// whose span holds `offset`, or `None` in the global scope.
fn enclosing_function_start(content: &str, offset: u32) -> Option<u32> {
    with_enclosing_function(content, offset, |found| found.map(|f| f.start))
}

/// The innermost function-like around an offset.
pub(super) struct EnclosingFunctionLike<'ast, 'arena> {
    start: u32,
    /// The closure or arrow function expression, when that is what it is.
    pub(super) closure: Option<&'ast Expression<'arena>>,
}

/// Hand `f` the innermost function-like whose span holds `offset`, or
/// `None` in the global scope.
pub(super) fn with_enclosing_function<T: Default>(
    content: &str,
    offset: u32,
    f: impl FnOnce(Option<&EnclosingFunctionLike<'_, '_>>) -> T,
) -> T {
    with_parsed_program(content, "enclosing_function_like", |program, _| {
        let mut state = Finder {
            target: offset,
            found: None,
        };
        for statement in program.statements.iter() {
            Walker::walk_statement(&FunctionFinder, statement, &mut state);
        }
        f(state.found.as_ref())
    })
}

struct Finder<'ast, 'arena> {
    target: u32,
    found: Option<EnclosingFunctionLike<'ast, 'arena>>,
}

impl<'ast, 'arena> Finder<'ast, 'arena> {
    /// Walking reaches outer function-likes first, so the last one that
    /// holds the target is the innermost.
    fn enter(&mut self, node: &impl HasSpan, closure: Option<&'ast Expression<'arena>>) {
        let span = node.span();
        if span.start.offset <= self.target && self.target < span.end.offset {
            self.found = Some(EnclosingFunctionLike {
                start: span.start.offset,
                closure,
            });
        }
    }
}

struct FunctionFinder;

impl<'ast, 'arena> Walker<'ast, 'arena, Finder<'ast, 'arena>> for FunctionFinder {
    crate::ast_walk::iterative_binary_walk!(Finder<'ast, 'arena>);

    fn walk_in_function(&self, node: &'ast Function<'arena>, ctx: &mut Finder<'ast, 'arena>) {
        ctx.enter(node, None);
    }

    fn walk_in_method(&self, node: &'ast Method<'arena>, ctx: &mut Finder<'ast, 'arena>) {
        ctx.enter(node, None);
    }

    fn walk_in_expression(&self, node: &'ast Expression<'arena>, ctx: &mut Finder<'ast, 'arena>) {
        if matches!(node, Expression::Closure(_) | Expression::ArrowFunction(_)) {
            ctx.enter(node, Some(node));
        }
    }
}
