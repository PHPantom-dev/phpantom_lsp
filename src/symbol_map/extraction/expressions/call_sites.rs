use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::*;

// ─── Call site emission ─────────────────────────────────────────────────────

/// Build and push a [`CallSite`] from an argument list and its call expression string.
pub(super) fn emit_call_site(
    call_expression: String,
    argument_list: &ArgumentList<'_>,
    call_sites: &mut Vec<CallSite>,
    untyped_closure_sites: &mut Vec<UntypedClosureSite>,
) {
    if call_expression.is_empty() {
        return;
    }
    let args_start = argument_list.left_parenthesis.end.offset;
    let args_end = argument_list.right_parenthesis.start.offset;
    let comma_offsets: Vec<u32> = argument_list
        .arguments
        .tokens
        .iter()
        .map(|t| t.start.offset)
        .collect();

    let arg_count = argument_list.arguments.len() as u32;

    // Collect the byte offset of each argument's start token and
    // track which arguments use named syntax (`name: value`).
    let mut arg_offsets = Vec::with_capacity(arg_count as usize);
    let mut named_arg_indices = Vec::new();
    let mut named_arg_names = Vec::new();
    let mut spread_arg_indices = Vec::new();
    for (i, arg) in argument_list.arguments.iter().enumerate() {
        match arg {
            Argument::Positional(pos) => {
                // If unpacking is used, the `...` token comes before the
                // value expression.  Use the ellipsis offset when present
                // so the hint appears before `...`.
                let offset = pos
                    .ellipsis
                    .as_ref()
                    .map(|e| e.start.offset)
                    .unwrap_or_else(|| pos.value.span().start.offset);
                arg_offsets.push(offset);
                if pos.ellipsis.is_some() {
                    spread_arg_indices.push(i as u32);
                }
            }
            Argument::Named(named) => {
                arg_offsets.push(named.name.span.start.offset);
                named_arg_indices.push(i as u32);
                named_arg_names.push(bytes_to_str(named.name.value).to_string());
            }
        }
    }

    // Detect argument unpacking (`...$args`).  Only positional
    // arguments can use the spread operator; the AST stores it as
    // `ellipsis: Some(Span)` on `PositionalArgument`.
    let has_unpacking = argument_list
        .arguments
        .iter()
        .any(|arg| matches!(arg, Argument::Positional(pos) if pos.ellipsis.is_some()));

    // Check arguments for closures/arrows with untyped parameters or
    // missing return types.
    for (arg_idx, arg) in argument_list.arguments.iter().enumerate() {
        let expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        collect_untyped_closure_site(expr, &call_expression, arg_idx, untyped_closure_sites);
    }

    call_sites.push(CallSite {
        args_start,
        args_end,
        call_expression,
        comma_offsets,
        arg_offsets,
        arg_count,
        has_unpacking,
        named_arg_indices,
        named_arg_names,
        spread_arg_indices,
    });
}

/// Build and push a [`CallSite`] from a partial argument list and its call expression string.
pub(in crate::symbol_map::extraction) fn emit_partial_call_site(
    call_expression: String,
    argument_list: &PartialArgumentList<'_>,
    call_sites: &mut Vec<CallSite>,
    untyped_closure_sites: &mut Vec<UntypedClosureSite>,
) {
    let args_start = argument_list.left_parenthesis.end.offset;
    let args_end = argument_list.right_parenthesis.start.offset;
    let comma_offsets = argument_list
        .arguments
        .tokens
        .iter()
        .map(|token| token.start.offset)
        .collect();
    let mut arg_offsets = Vec::with_capacity(argument_list.arguments.len());
    let mut named_arg_indices = Vec::new();
    let mut named_arg_names = Vec::new();
    let mut spread_arg_indices = Vec::new();

    for (index, argument) in argument_list.arguments.iter().enumerate() {
        match argument {
            PartialArgument::Positional(argument) => {
                let offset = argument
                    .ellipsis
                    .map(|span| span.start.offset)
                    .unwrap_or_else(|| argument.value.span().start.offset);
                arg_offsets.push(offset);
                if argument.ellipsis.is_some() {
                    spread_arg_indices.push(index as u32);
                }
                collect_untyped_closure_site(
                    argument.value,
                    &call_expression,
                    index,
                    untyped_closure_sites,
                );
            }
            PartialArgument::Named(argument) => {
                arg_offsets.push(argument.name.span.start.offset);
                named_arg_indices.push(index as u32);
                named_arg_names.push(bytes_to_str(argument.name.value).to_string());
                collect_untyped_closure_site(
                    argument.value,
                    &call_expression,
                    index,
                    untyped_closure_sites,
                );
            }
            PartialArgument::NamedPlaceholder(argument) => {
                arg_offsets.push(argument.name.span.start.offset);
                named_arg_indices.push(index as u32);
                named_arg_names.push(bytes_to_str(argument.name.value).to_string());
            }
            PartialArgument::Placeholder(argument) => arg_offsets.push(argument.span.start.offset),
            PartialArgument::VariadicPlaceholder(argument) => {
                arg_offsets.push(argument.span.start.offset)
            }
        }
    }

    call_sites.push(CallSite {
        args_start,
        args_end,
        call_expression,
        comma_offsets,
        arg_count: argument_list.arguments.len() as u32,
        has_unpacking: !spread_arg_indices.is_empty(),
        arg_offsets,
        named_arg_indices,
        named_arg_names,
        spread_arg_indices,
    });
}

/// If `expr` is a closure or arrow function, collect an [`UntypedClosureSite`]
/// with its untyped parameters and (optionally) its close-paren offset for a
/// return type hint.
fn collect_untyped_closure_site(
    expr: &Expression<'_>,
    parent_call_expression: &str,
    arg_index: usize,
    out: &mut Vec<UntypedClosureSite>,
) {
    let (params, close_paren_offset, has_return_type) = match expr {
        Expression::Closure(c) => (
            &c.parameter_list.parameters,
            c.parameter_list.span().end.offset,
            c.return_type_hint.is_some(),
        ),
        Expression::ArrowFunction(a) => (
            &a.parameter_list.parameters,
            a.parameter_list.span().end.offset,
            a.return_type_hint.is_some(),
        ),
        _ => return,
    };

    let mut untyped_params = Vec::new();
    for (param_idx, param) in params.iter().enumerate() {
        if param.hint.is_none() {
            untyped_params.push((param_idx, param.variable.span.start.offset));
        }
    }

    // Only emit a site if there is something for inlay hints to show:
    // untyped parameters or a missing return type.
    if untyped_params.is_empty() && has_return_type {
        return;
    }

    out.push(UntypedClosureSite {
        parent_call_expression: parent_call_expression.to_string(),
        arg_index_in_parent: arg_index,
        close_paren_offset: if has_return_type {
            None
        } else {
            Some(close_paren_offset)
        },
        untyped_params,
    });
}
