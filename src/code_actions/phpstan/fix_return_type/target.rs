//! The function a PHPStan return-type diagnostic is about, found in the
//! parsed file.
//!
//! PHPStan reports a mismatched `return` on the line the statement starts
//! on, or for an arrow function on the line of the expression it returns,
//! and a problem with a declared return type on the line its declaration
//! starts on.  A closure can share either line with the function it is
//! written in, so the message settles which of them is meant: PHPStan calls
//! a closure or an arrow function an "Anonymous function", and a function or
//! method by its name.

use std::ops::RangeInclusive;

use mago_span::{HasSpan, Span};
use mago_syntax::cst::*;
use tower_lsp::lsp_types::TextEdit;

use crate::code_actions::FunctionDocblock;
use crate::code_actions::cursor_context::FunctionLike;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::text_position::{byte_range_to_lsp_range, line_start_byte_offset};

/// Where on its function PHPStan reports a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code_actions::phpstan) enum Anchor {
    /// The line a `return` statement starts on, or the line an arrow
    /// function's expression starts on.
    Return,
    /// A line of the declaration, from its first attribute or modifier to
    /// where its body starts.
    Declaration,
}

/// The function-like declaration a return-type diagnostic is about, as
/// positions in the content it was found in.
pub(in crate::code_actions::phpstan) struct TargetFunction {
    /// Where the declaration starts, at its first attribute, modifier or
    /// keyword.
    pub(in crate::code_actions::phpstan) start: u32,
    /// Whether it is a closure or an arrow function.  PHPStan reads no
    /// docblock on one, so a fix only keeps a `@return` tag it already has
    /// in step and never writes a new one.
    pub(in crate::code_actions::phpstan) anonymous: bool,
    /// Just past the `)` that closes its parameter list, or past the `use`
    /// clause of a closure: where its return type is declared.
    params_end: u32,
    /// The `:` of its native return type and the type after it.
    return_type: Option<(Span, Span)>,
    /// The `}` that closes its body.  An arrow function and an abstract
    /// method have none.
    pub(in crate::code_actions::phpstan) body_end: Option<u32>,
    /// Its docblock, or where a new one goes.
    pub(in crate::code_actions::phpstan) docblock: FunctionDocblock,
    /// The `return` statement the diagnostic is on, for one reported on a
    /// statement rather than on an arrow function's expression.
    pub(in crate::code_actions::phpstan) statement: Option<ReturnStatement>,
}

/// A `return` statement in the function a diagnostic is about.
pub(in crate::code_actions::phpstan) struct ReturnStatement {
    /// Where the `return` keyword starts.
    pub(in crate::code_actions::phpstan) start: u32,
    /// Just past the `;` that ends it, unless a closing tag ends it instead.
    pub(in crate::code_actions::phpstan) end: Option<u32>,
    /// The value it returns.
    pub(in crate::code_actions::phpstan) value: Option<Span>,
    /// Whether that value is the `null` literal.
    pub(in crate::code_actions::phpstan) returns_null: bool,
    /// For a `return` that is the whole body of an `if`, `elseif`, `else`,
    /// loop or `declare` written without braces, where the head of that
    /// construct ends: just past its `)`, `else` or `do`.
    pub(in crate::code_actions::phpstan) braceless_after: Option<u32>,
    /// Whether a loop in its function holds it, so that the function goes
    /// on past it even when no statement follows.
    pub(in crate::code_actions::phpstan) in_loop: bool,
}

/// A `return` statement [`find_target`] found a function by, with what
/// holds it.
struct FoundReturn<'ast, 'arena> {
    statement: &'ast Return<'arena>,
    braceless_after: Option<u32>,
    in_loop: bool,
}

impl TargetFunction {
    fn new(
        program: &Program<'_>,
        content: &str,
        function: FunctionLike<'_, '_>,
        found: Option<FoundReturn<'_, '_>>,
    ) -> Self {
        let span = function.span();
        let params_end = match function {
            FunctionLike::Closure(Closure {
                use_clause: Some(use_clause),
                ..
            }) => use_clause.right_parenthesis,
            _ => function.parameter_list().right_parenthesis,
        };
        Self {
            start: span.start.offset,
            anonymous: function.is_anonymous(),
            params_end: params_end.end.offset,
            return_type: function
                .return_type_hint()
                .map(|return_type| (return_type.colon, return_type.hint.span())),
            body_end: function.body().map(|block| block.right_brace.start.offset),
            docblock: FunctionDocblock::of(program, content, span),
            statement: found.map(|found| ReturnStatement {
                start: found.statement.r#return.span.start.offset,
                end: match found.statement.terminator {
                    Terminator::Semicolon(semicolon) => Some(semicolon.end.offset),
                    _ => None,
                },
                value: found.statement.value.map(|value| value.span()),
                returns_null: matches!(
                    found.statement.value,
                    Some(Expression::Literal(Literal::Null(_)))
                ),
                braceless_after: found.braceless_after,
                in_loop: found.in_loop,
            }),
        }
    }

    /// Whether it declares a native return type.
    pub(in crate::code_actions::phpstan) fn has_return_type(&self) -> bool {
        self.return_type.is_some()
    }

    /// The native return type, as written.
    pub(in crate::code_actions::phpstan) fn native_return_type(
        &self,
        content: &str,
    ) -> Option<PhpType> {
        let (_, hint) = self.return_type?;
        Some(PhpType::parse(
            &content[hint.start.offset as usize..hint.end.offset as usize],
        ))
    }

    /// The edit that declares `return_type` in place of the native return
    /// type.  `None` when there is none, or it already reads `return_type`.
    pub(in crate::code_actions::phpstan) fn replace_return_type_edit(
        &self,
        content: &str,
        return_type: &str,
    ) -> Option<TextEdit> {
        let (colon, hint) = self.return_type?;
        let written = &content[hint.start.offset as usize..hint.end.offset as usize];
        (written != return_type).then(|| TextEdit {
            range: byte_range_to_lsp_range(
                content,
                colon.start.offset as usize,
                hint.end.offset as usize,
            ),
            new_text: format!(": {return_type}"),
        })
    }

    /// The edit that declares `return_type` on a function that has no
    /// return type.
    pub(in crate::code_actions::phpstan) fn add_return_type_edit(
        &self,
        content: &str,
        return_type: &str,
    ) -> TextEdit {
        let at = self.params_end as usize;
        TextEdit {
            range: byte_range_to_lsp_range(content, at, at),
            new_text: format!(": {return_type}"),
        }
    }
}

/// The function a diagnostic with `message`, reported at `anchor` on
/// `line`, is about.
pub(in crate::code_actions::phpstan) fn target_function(
    content: &str,
    line: usize,
    anchor: Anchor,
    message: &str,
) -> Option<TargetFunction> {
    let start = line_start_byte_offset(content, line);
    let text = content.get(start..)?.lines().next()?;
    let line = start as u32..=(start + text.len()) as u32;
    let anonymous = message.starts_with("Anonymous function");
    with_parsed_program(content, "fix_return_type_target", |program, content| {
        let (function, statement) = find_target(program, line, anchor, anonymous)?;
        Some(TargetFunction::new(program, content, function, statement))
    })
}

/// The first function-like, in source order and outer before inner, that
/// a diagnostic at `anchor` on `line` can be about: a closure or arrow
/// function when `anonymous`, else a function or method.  For a
/// [`Anchor::Return`], also the `return` statement it was found by, unless
/// that is an arrow function's expression.
///
/// Only the nodes that reach onto the line are visited.
fn find_target<'ast, 'arena>(
    program: &'ast Program<'arena>,
    line: RangeInclusive<u32>,
    anchor: Anchor,
    anonymous: bool,
) -> Option<(
    FunctionLike<'ast, 'arena>,
    Option<FoundReturn<'ast, 'arena>>,
)> {
    let crosses_line = |start: u32, end: u32| start <= *line.end() && *line.start() <= end;
    let wanted = |function: FunctionLike<'_, '_>| function.is_anonymous() == anonymous;

    // Each node comes with the function its `return` statements belong to:
    // none at the top level or in a property hook, which returns the
    // property's type and has no return type of its own.  Then whether a
    // loop of that function holds it, and for a braceless body, where the
    // head of the construct it is the body of ends.
    let mut pending = vec![(Node::Program(program), None, false, None)];
    let mut children = Vec::new();
    while let Some((node, owner, in_loop, braceless_after)) = pending.pop() {
        let mut in_loop = in_loop;
        let owner = if let Some(function) = FunctionLike::of(node) {
            in_loop = false;
            let reported_here = match anchor {
                Anchor::Declaration => {
                    crosses_line(function.span().start.offset, body_start(function))
                }
                Anchor::Return => matches!(
                    function,
                    FunctionLike::ArrowFunction(arrow)
                        if line.contains(&arrow.expression.span().start.offset)
                ),
            };
            if reported_here && wanted(function) {
                return Some((function, None));
            }
            Some(function)
        } else if let Node::PropertyHook(_) = node {
            None
        } else {
            if anchor == Anchor::Return
                && let Node::Return(statement) = node
                && line.contains(&statement.span().start.offset)
                && let Some(function) = owner
                && wanted(function)
            {
                let found = FoundReturn {
                    statement,
                    braceless_after,
                    in_loop,
                };
                return Some((function, Some(found)));
            }
            owner
        };
        in_loop |= matches!(
            node,
            Node::While(_) | Node::DoWhile(_) | Node::For(_) | Node::Foreach(_)
        );
        let braceless_after = braceless_body_after(node, braceless_after);
        node.visit_children(|child| {
            let span = child.span();
            if crosses_line(span.start.offset, span.end.offset) {
                children.push(child);
            }
        });
        pending.extend(
            children
                .drain(..)
                .rev()
                .map(|child| (child, owner, in_loop, braceless_after)),
        );
    }
    None
}

/// For the children of `node`, where the head of the construct ends that
/// a child is the braceless body of, given `braceless_after` for `node`
/// itself.  Only a statement child, and the `return` it wraps, can be such
/// a body; a block or colon-delimited body passes nothing on.
fn braceless_body_after(node: Node<'_, '_>, braceless_after: Option<u32>) -> Option<u32> {
    match node {
        Node::If(If {
            right_parenthesis, ..
        })
        | Node::IfStatementBodyElseIfClause(IfStatementBodyElseIfClause {
            right_parenthesis,
            ..
        })
        | Node::While(While {
            right_parenthesis, ..
        })
        | Node::For(For {
            right_parenthesis, ..
        })
        | Node::Foreach(Foreach {
            right_parenthesis, ..
        })
        | Node::Declare(Declare {
            right_parenthesis, ..
        }) => Some(right_parenthesis.end.offset),
        Node::IfStatementBodyElseClause(clause) => Some(clause.r#else.span.end.offset),
        Node::DoWhile(do_while) => Some(do_while.r#do.span.end.offset),
        Node::IfBody(_)
        | Node::IfStatementBody(_)
        | Node::WhileBody(_)
        | Node::ForBody(_)
        | Node::ForeachBody(_)
        | Node::DeclareBody(_)
        | Node::Statement(_) => braceless_after,
        _ => None,
    }
}

/// Where the body of `function` starts, which ends its declaration: the
/// `{` of its block, the `=>` of an arrow function, or the end of an
/// abstract method.
fn body_start(function: FunctionLike<'_, '_>) -> u32 {
    match function {
        FunctionLike::ArrowFunction(arrow) => arrow.arrow.start.offset,
        _ => function.body().map_or(function.span().end.offset, |block| {
            block.left_brace.start.offset
        }),
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
