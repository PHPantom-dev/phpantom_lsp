use mago_span::HasSpan;

use super::*;

// ─── Call site emission ─────────────────────────────────────────────────────

/// Build and push a [`CallSite`] from an argument list and its call expression.
pub(super) fn emit_call_site(
    call_expression: CallText,
    argument_list: &ArgumentList<'_>,
    call_sites: &mut CallSites,
    untyped_closure_sites: &mut Vec<UntypedClosureSite>,
) {
    let expression = match &call_expression {
        CallText::Owned(text) if text.is_empty() => return,
        CallText::Owned(text) => CallExpression::Text(text),
        CallText::Arena { start, len } => CallExpression::Stored {
            start: *start,
            len: *len,
        },
    };
    let args = argument_list.arguments.iter().map(|arg| match arg {
        // The hint for a spread goes before its `...`.
        Argument::Positional(pos) => match &pos.ellipsis {
            Some(ellipsis) => CallArg {
                offset: ellipsis.start.offset,
                kind: CallArgKind::Spread,
            },
            None => CallArg {
                offset: pos.value.span().start.offset,
                kind: CallArgKind::Positional,
            },
        },
        Argument::Named(named) => CallArg {
            offset: named.name.span.start.offset,
            kind: CallArgKind::Named(bytes_to_str(named.name.value)),
        },
    });
    call_sites.push(
        argument_list.left_parenthesis.end.offset,
        argument_list.right_parenthesis.start.offset,
        expression,
        argument_list
            .arguments
            .tokens
            .iter()
            .map(|t| t.start.offset),
        args,
    );
    let call_expression = match &call_expression {
        CallText::Owned(text) => text.as_str(),
        CallText::Arena { start, len } => call_sites.stored_text(*start, *len),
    };

    // Check arguments for closures/arrows with untyped parameters or
    // missing return types.
    for (arg_idx, arg) in argument_list.arguments.iter().enumerate() {
        let expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        collect_untyped_closure_site(expr, call_expression, arg_idx, untyped_closure_sites);
    }
}

/// Build and push a [`CallSite`] from a partial argument list and its call expression string.
pub(in crate::symbol_map::extraction) fn emit_partial_call_site(
    call_expression: String,
    argument_list: &PartialArgumentList<'_>,
    call_sites: &mut CallSites,
    untyped_closure_sites: &mut Vec<UntypedClosureSite>,
) {
    let args = argument_list
        .arguments
        .iter()
        .map(|argument| match argument {
            PartialArgument::Positional(argument) => match argument.ellipsis {
                Some(ellipsis) => CallArg {
                    offset: ellipsis.start.offset,
                    kind: CallArgKind::Spread,
                },
                None => CallArg {
                    offset: argument.value.span().start.offset,
                    kind: CallArgKind::Positional,
                },
            },
            PartialArgument::Named(argument) => CallArg {
                offset: argument.name.span.start.offset,
                kind: CallArgKind::Named(bytes_to_str(argument.name.value)),
            },
            PartialArgument::NamedPlaceholder(argument) => CallArg {
                offset: argument.name.span.start.offset,
                kind: CallArgKind::Named(bytes_to_str(argument.name.value)),
            },
            PartialArgument::Placeholder(argument) => CallArg {
                offset: argument.span.start.offset,
                kind: CallArgKind::Positional,
            },
            PartialArgument::VariadicPlaceholder(argument) => CallArg {
                offset: argument.span.start.offset,
                kind: CallArgKind::Positional,
            },
        });
    call_sites.push(
        argument_list.left_parenthesis.end.offset,
        argument_list.right_parenthesis.start.offset,
        CallExpression::Text(&call_expression),
        argument_list
            .arguments
            .tokens
            .iter()
            .map(|token| token.start.offset),
        args,
    );

    for (index, argument) in argument_list.arguments.iter().enumerate() {
        let value = match argument {
            PartialArgument::Positional(argument) => argument.value,
            PartialArgument::Named(argument) => argument.value,
            _ => continue,
        };
        collect_untyped_closure_site(value, &call_expression, index, untyped_closure_sites);
    }
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
