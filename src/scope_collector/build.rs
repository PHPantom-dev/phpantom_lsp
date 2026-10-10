//! Public constructors that drive the [`Collector`] over a function,
//! method, closure, or an arbitrary statement list and return a
//! [`ScopeMap`].

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::atom::bytes_to_str;

use super::collector::{Collector, walk_expression, walk_statement};
use super::scope_map::*;

/// The body of a variable scope.
///
/// Most scopes are a statement list, but an arrow-form property hook
/// (`get => $this->first . $this->last;`) is a single expression that
/// still has its own scope, parameters, and variable accesses.
#[derive(Clone, Copy)]
pub(crate) enum ScopeBody<'ast, 'arena> {
    Statements(&'ast [Statement<'arena>]),
    Expression(&'ast Expression<'arena>),
}

/// The variables a scope's signature introduces.
#[derive(Clone, Copy)]
struct ScopeParams<'ast, 'arena> {
    /// The declared parameter list, absent for a property hook that
    /// spells none out (`get { … }`).
    declared: Option<&'ast FunctionLikeParameterList<'arena>>,
    /// Variables the language hands the body without a declaration: a
    /// `set` hook's `$value`.
    implicit: &'static [&'static str],
}

impl<'ast, 'arena> ScopeBody<'ast, 'arena> {
    /// Drive a [`Walker`] visitor over the body, whichever spelling it
    /// has.  Consumers that scan a scope for a feature (`extract()`,
    /// `compact()`, `isset()` guards, …) use this so an expression-bodied
    /// hook is scanned exactly like a statement-bodied one.
    pub(crate) fn walk_with<C, W: Walker<'ast, 'arena, C>>(&self, walker: &W, context: &mut C) {
        match self {
            ScopeBody::Statements(statements) => {
                for stmt in statements.iter() {
                    walker.walk_statement(stmt, context);
                }
            }
            ScopeBody::Expression(expr) => walker.walk_expression(expr, context),
        }
    }

    /// Collect the body's variable accesses into `collector`.
    fn collect_into(&self, collector: &mut Collector<'_>) {
        match self {
            ScopeBody::Statements(statements) => {
                for stmt in statements.iter() {
                    walk_statement(stmt, collector);
                }
            }
            ScopeBody::Expression(expr) => walk_expression(expr, collector),
        }
    }
}

/// Build a [`ScopeMap`] for the function/method/closure body that
/// contains `offset`.  Walks top-level and namespaced statements to
/// find the enclosing body, then collects variable accesses within it.
///
/// This is the shared implementation behind the `build_scope_map`
/// helpers in extract-function, extract-variable, and inline-variable
/// code actions.  All three need the same "find enclosing scope, then
/// collect" pattern.
pub(crate) fn build_scope_map_for_offset(
    statements: &[Statement<'_>],
    offset: u32,
    content_len: u32,
) -> ScopeMap {
    for stmt in statements {
        if let Some(map) = try_build_scope_from_statement(stmt, offset) {
            return map;
        }
    }
    // Fallback: top-level scope.
    collect_scope(statements, 0, content_len)
}

/// Recursively try to build a scope map from a single statement that
/// contains `offset`.
fn try_build_scope_from_statement(stmt: &Statement<'_>, offset: u32) -> Option<ScopeMap> {
    match stmt {
        Statement::Function(func) => {
            let body_start = func.body.left_brace.start.offset;
            let body_end = func.body.right_brace.end.offset;
            if offset >= body_start && offset <= body_end {
                return Some(collect_function_scope_with_kind(
                    &func.parameter_list,
                    func.body.statements.as_slice(),
                    body_start,
                    body_end,
                    FrameKind::Function,
                ));
            }
        }
        Statement::Class(class) => {
            return try_build_scope_from_members(class.members.as_slice(), offset);
        }
        Statement::Trait(tr) => {
            return try_build_scope_from_members(tr.members.as_slice(), offset);
        }
        Statement::Enum(en) => {
            return try_build_scope_from_members(en.members.as_slice(), offset);
        }
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                if let Some(map) = try_build_scope_from_statement(inner, offset) {
                    return Some(map);
                }
            }
        }
        _ => {}
    }
    None
}

/// Try to build a scope map from the members of a class, trait, or enum
/// declaration: method bodies plus the `get`/`set` bodies of hooked
/// properties, including properties promoted in the constructor's
/// parameter list.
fn try_build_scope_from_members(members: &[ClassLikeMember<'_>], offset: u32) -> Option<ScopeMap> {
    for member in members.iter() {
        match member {
            ClassLikeMember::Method(method) => {
                // A constructor-promoted property carries its hooks in
                // the parameter list, outside the method body.
                for param in method.parameter_list.parameters.iter() {
                    if let Some(hooks) = &param.hooks
                        && let Some(map) = try_build_scope_from_hooks(hooks, offset)
                    {
                        return Some(map);
                    }
                }

                if let MethodBody::Concrete(block) = &method.body {
                    let body_start = block.left_brace.start.offset;
                    let body_end = block.right_brace.end.offset;
                    if offset >= body_start && offset <= body_end {
                        return Some(collect_function_scope_with_kind(
                            &method.parameter_list,
                            block.statements.as_slice(),
                            body_start,
                            body_end,
                            FrameKind::Method,
                        ));
                    }
                }
            }
            ClassLikeMember::Property(Property::Hooked(hooked)) => {
                if let Some(map) = try_build_scope_from_hooks(&hooked.hook_list, offset) {
                    return Some(map);
                }
            }
            _ => {}
        }
    }
    None
}

/// Try to build a scope map from the hook of `hooks` that contains
/// `offset`.
fn try_build_scope_from_hooks(hooks: &PropertyHookList<'_>, offset: u32) -> Option<ScopeMap> {
    for hook in hooks.hooks.iter() {
        let Some((body_start, body_end, body)) = hook_body_span(hook) else {
            continue;
        };
        if offset >= body_start && offset <= body_end {
            return Some(collect_hook_scope(hook, body, body_start, body_end));
        }
    }
    None
}

/// The byte range and body of a concrete property hook.
///
/// A block hook scopes from its opening brace, an expression hook from
/// its arrow — the same rule as a method body, whose scope starts at `{`
/// and leaves the parameter list outside.
pub(crate) fn hook_body_span<'ast, 'arena>(
    hook: &'ast PropertyHook<'arena>,
) -> Option<(u32, u32, ScopeBody<'ast, 'arena>)> {
    let PropertyHookBody::Concrete(body) = &hook.body else {
        return None;
    };
    Some(match body {
        PropertyHookConcreteBody::Block(block) => (
            block.left_brace.start.offset,
            block.right_brace.end.offset,
            ScopeBody::Statements(block.statements.as_slice()),
        ),
        PropertyHookConcreteBody::Expression(expr_body) => (
            expr_body.arrow.start.offset,
            expr_body.semicolon.end.offset,
            ScopeBody::Expression(expr_body.expression),
        ),
    })
}

/// Collect the scope of a single property hook body.
///
/// A hook is never static, so `$this` is always in scope; a `set` hook
/// that spells out no parameter list still receives `$value`.
fn collect_hook_scope<'a>(
    hook: &PropertyHook<'a>,
    body: ScopeBody<'_, 'a>,
    body_start: u32,
    body_end: u32,
) -> ScopeMap {
    collect_hook_scope_with_resolver(hook, body, body_start, body_end, None, None)
}

/// Like [`collect_hook_scope`] but accepts an optional [`ByRefResolver`]
/// callback and the name of the class the hook is declared in.
pub(crate) fn collect_hook_scope_with_resolver<'a>(
    hook: &PropertyHook<'a>,
    body: ScopeBody<'_, 'a>,
    body_start: u32,
    body_end: u32,
    resolver: Option<ByRefResolver<'_>>,
    enclosing_class_name: Option<String>,
) -> ScopeMap {
    let implicit: &'static [&'static str] =
        if hook.parameter_list.is_none() && hook.name.value.eq_ignore_ascii_case(b"set") {
            &["$value"]
        } else {
            &[]
        };

    collect_scope_with_kind_and_resolver(
        ScopeParams {
            declared: hook.parameter_list.as_ref(),
            implicit,
        },
        body,
        body_start,
        body_end,
        FrameKind::Method,
        resolver,
        enclosing_class_name,
    )
}

/// Collect all variable reads and writes within a function/method body.
///
/// `body_start` and `body_end` are the byte offsets of the opening `{`
/// and closing `}` of the function body.  The returned [`ScopeMap`]
/// contains a single top-level frame plus any nested frames (closures,
/// arrow functions, catch blocks).
pub(crate) fn collect_scope(
    statements: &[Statement<'_>],
    body_start: u32,
    body_end: u32,
) -> ScopeMap {
    collect_scope_with_resolver(statements, body_start, body_end, None)
}

/// Like [`collect_scope`] but accepts an optional [`ByRefResolver`]
/// callback for detecting by-reference parameters in user-defined
/// function and static method calls.
pub(crate) fn collect_scope_with_resolver(
    statements: &[Statement<'_>],
    body_start: u32,
    body_end: u32,
    resolver: Option<ByRefResolver<'_>>,
) -> ScopeMap {
    let mut collector = match resolver {
        Some(r) => Collector::with_resolver(r),
        None => Collector::new(),
    };

    collector.push_frame(Frame {
        start: body_start,
        end: body_end,
        kind: FrameKind::TopLevel,
        parameters: Vec::new(),
        is_static: false,
    });

    for stmt in statements {
        walk_statement(stmt, &mut collector);
    }

    collector.pop_frame();

    collector.frames.sort_by_key(|f| f.start);

    ScopeMap {
        accesses: collector.accesses,
        frames: collector.frames,
        has_this_or_self: collector.has_this_or_self,
        reference_bindings: collector.reference_bindings,
        closure_use_lists: collector.closure_use_lists,
        dynamic_scope_accesses: collector.dynamic_scope_accesses,
    }
}

/// Collect scope information for a set of function parameters.
///
/// Records each parameter as a `Write` access at its offset, and each
/// `&$param` as a reference binding live for the whole body.
///
/// The frame the parameters belong to must already be pushed.
fn collect_parameters(params: &FunctionLikeParameterList<'_>, collector: &mut Collector<'_>) {
    for param in params.parameters.iter() {
        let name = bytes_to_str(param.variable.name).to_string();
        let offset = param.variable.span().start.offset;
        collector.push_access_with_role(
            name.clone(),
            offset,
            AccessKind::Write,
            AccessRole::Parameter,
        );
        if param.ampersand.is_some() {
            collector.push_reference_binding(name, offset);
        }
        if let Some(ref default) = param.default_value {
            let mut tmp = Collector::new();
            walk_expression(default.value, &mut tmp);
            collector.accesses.extend(tmp.accesses);
        }
    }
}

/// Convenience: collect scope from a full method or function AST node.
///
/// Includes parameter declarations and the body.
pub(crate) fn collect_function_scope<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
    body_start: u32,
    body_end: u32,
) -> ScopeMap {
    collect_function_scope_with_kind(params, body, body_start, body_end, FrameKind::Function)
}

/// Like [`collect_function_scope`] but accepts an optional
/// [`ByRefResolver`] callback.
pub(crate) fn collect_function_scope_with_resolver<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
    body_start: u32,
    body_end: u32,
    resolver: Option<ByRefResolver<'_>>,
) -> ScopeMap {
    collect_function_scope_with_kind_and_resolver(
        params,
        ScopeBody::Statements(body),
        body_start,
        body_end,
        FrameKind::Function,
        resolver,
        None,
    )
}

/// Like [`collect_function_scope`] but allows specifying the
/// [`FrameKind`] for the outermost frame.  Use `FrameKind::Method`
/// when collecting inside a class method.
pub(crate) fn collect_function_scope_with_kind<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
    body_start: u32,
    body_end: u32,
    kind: FrameKind,
) -> ScopeMap {
    collect_function_scope_with_kind_and_resolver(
        params,
        ScopeBody::Statements(body),
        body_start,
        body_end,
        kind,
        None,
        None,
    )
}

/// Like [`collect_function_scope_with_kind`] but accepts an optional
/// [`ByRefResolver`] callback for detecting by-reference parameters
/// in user-defined function and static method calls.
pub(crate) fn collect_function_scope_with_kind_and_resolver<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: ScopeBody<'_, 'a>,
    body_start: u32,
    body_end: u32,
    kind: FrameKind,
    resolver: Option<ByRefResolver<'_>>,
    enclosing_class_name: Option<String>,
) -> ScopeMap {
    collect_scope_with_kind_and_resolver(
        ScopeParams {
            declared: Some(params),
            implicit: &[],
        },
        body,
        body_start,
        body_end,
        kind,
        resolver,
        enclosing_class_name,
    )
}

/// The shared implementation behind every `collect_*_scope` helper.
///
/// `params` is optional because a property hook may declare no
/// parameter list at all (`get { … }`), and `implicit_params` carries
/// the variables such a hook is handed anyway (a `set` hook's `$value`).
fn collect_scope_with_kind_and_resolver<'a>(
    params: ScopeParams<'_, 'a>,
    body: ScopeBody<'_, 'a>,
    body_start: u32,
    body_end: u32,
    kind: FrameKind,
    resolver: Option<ByRefResolver<'_>>,
    enclosing_class_name: Option<String>,
) -> ScopeMap {
    let mut collector = match resolver {
        Some(r) => Collector::with_resolver(r),
        None => Collector::new(),
    };
    collector.enclosing_class_name = enclosing_class_name;

    let param_names: Vec<String> = params
        .declared
        .iter()
        .flat_map(|p| p.parameters.iter())
        .map(|p| bytes_to_str(p.variable.name).to_string())
        .chain(params.implicit.iter().map(|n| (*n).to_string()))
        .collect();

    collector.push_frame(Frame {
        start: body_start,
        end: body_end,
        kind,
        parameters: param_names,
        is_static: false,
    });

    // Record parameters as writes.
    if let Some(declared) = params.declared {
        collect_parameters(declared, &mut collector);
    }

    body.collect_into(&mut collector);

    collector.pop_frame();

    collector.frames.sort_by_key(|f| f.start);

    ScopeMap {
        accesses: collector.accesses,
        frames: collector.frames,
        has_this_or_self: collector.has_this_or_self,
        reference_bindings: collector.reference_bindings,
        closure_use_lists: collector.closure_use_lists,
        dynamic_scope_accesses: collector.dynamic_scope_accesses,
    }
}
