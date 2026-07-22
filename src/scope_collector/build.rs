//! Public constructors that drive the [`Collector`] over a function,
//! method, closure, or an arbitrary statement list and return a
//! [`ScopeMap`].

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;

use super::collector::{Collector, walk_expression, walk_statement};
use super::scope_map::*;

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
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
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
        }
        Statement::Trait(tr) => {
            for member in tr.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
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
        }
        Statement::Enum(en) => {
            for member in en.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
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
        captures: Vec::new(),
        parameters: Vec::new(),
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
        has_reference_params: collector.has_reference_params,
    }
}

/// Collect scope information for a set of function parameters.
///
/// Records each parameter as a `Write` access at its offset.
pub(crate) fn collect_parameters(
    params: &FunctionLikeParameterList<'_>,
    collector_accesses: &mut Vec<VarAccess>,
    collector_has_reference: &mut bool,
) {
    for param in params.parameters.iter() {
        let name = bytes_to_str(param.variable.name).to_string();
        let offset = param.variable.span().start.offset;
        collector_accesses.push(VarAccess {
            name,
            offset,
            kind: AccessKind::Write,
        });
        if param.ampersand.is_some() {
            *collector_has_reference = true;
        }
        if let Some(ref default) = param.default_value {
            let mut tmp = Collector::new();
            walk_expression(default.value, &mut tmp);
            collector_accesses.extend(tmp.accesses);
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
        body,
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
        params, body, body_start, body_end, kind, None, None,
    )
}

/// Like [`collect_function_scope_with_kind`] but accepts an optional
/// [`ByRefResolver`] callback for detecting by-reference parameters
/// in user-defined function and static method calls.
pub(crate) fn collect_function_scope_with_kind_and_resolver<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
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
        .parameters
        .iter()
        .map(|p| bytes_to_str(p.variable.name).to_string())
        .collect();

    collector.push_frame(Frame {
        start: body_start,
        end: body_end,
        kind,
        captures: Vec::new(),
        parameters: param_names,
    });

    // Record parameters as writes.
    collect_parameters(
        params,
        &mut collector.accesses,
        &mut collector.has_reference_params,
    );

    for stmt in body {
        walk_statement(stmt, &mut collector);
    }

    collector.pop_frame();

    collector.frames.sort_by_key(|f| f.start);

    ScopeMap {
        accesses: collector.accesses,
        frames: collector.frames,
        has_this_or_self: collector.has_this_or_self,
        has_reference_params: collector.has_reference_params,
    }
}
