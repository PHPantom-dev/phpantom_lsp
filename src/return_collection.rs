//! Shared AST walker for collecting `return` statement expressions
//! from a function or method body.
//!
//! Recurses into control-flow blocks (if/else, for, while, switch,
//! try/catch/finally, block, declare) but never into a nested
//! function declaration — closures and arrow functions are
//! expressions, so a statement-level walk never visits their bodies.
//!
//! Used by the return-type-mismatch diagnostic
//! ([`crate::diagnostics::return_type_errors`]) and by return-type
//! inference for code actions
//! ([`crate::code_actions::phpstan::fix_return_type`]).

use mago_span::HasSpan;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::statement::Statement;

use crate::parser::for_each_nested_statement_list;

/// Collect `(expression, start_offset, end_offset, stmt_start_offset)` for
/// every `return` reachable from `stmts` without crossing into a nested
/// function/closure scope.  Bare `return;` yields `expression: None` with
/// the span of the `return` keyword itself.  `stmt_start_offset` is the
/// start of the `return` keyword, which callers use to look up a
/// standalone docblock preceding the statement — the expression's own
/// start offset sits after the keyword, so it cannot be used for that
/// lookup.
pub(crate) fn collect_returns<'a>(
    stmts: impl Iterator<Item = &'a Statement<'a>>,
    returns: &mut Vec<(Option<&'a Expression<'a>>, usize, usize, usize)>,
) {
    for stmt in stmts {
        collect_returns_from_stmt(stmt, returns);
    }
}

fn collect_returns_from_stmt<'a>(
    stmt: &Statement<'a>,
    returns: &mut Vec<(Option<&'a Expression<'a>>, usize, usize, usize)>,
) {
    match stmt {
        Statement::Return(ret) => {
            let stmt_start = ret.r#return.span.start.offset as usize;
            if let Some(val) = ret.value {
                let span = val.span();
                returns.push((
                    Some(val),
                    span.start.offset as usize,
                    span.end.offset as usize,
                    stmt_start,
                ));
            } else {
                // Bare `return;` — use the `return` keyword span.
                let kw_span = ret.r#return.span;
                returns.push((
                    None,
                    kw_span.start.offset as usize,
                    kw_span.end.offset as usize,
                    stmt_start,
                ));
            }
        }
        // Never recurses into a nested function or class: their returns
        // belong to their own bodies.
        _ => for_each_nested_statement_list(stmt, |nested| {
            for inner in nested {
                collect_returns_from_stmt(inner, returns);
            }
        }),
    }
}
