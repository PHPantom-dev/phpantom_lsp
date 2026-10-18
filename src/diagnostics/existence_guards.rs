//! The constructs that make a missing symbol legitimate: an
//! `isset()`/`empty()` argument, and an existence check
//! (`function_exists`, `class_exists`, `method_exists`) a call is written
//! under.
//!
//! The callers only judge spans the symbol map recorded, which come from
//! the same parse, so these are read from the AST.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::helpers::ByteRange;
use crate::atom::{bytes_to_str, literal_bytes_to_str};

/// Compute the byte ranges of `isset(...)` and `empty(...)` argument lists.
///
/// A member or array-index access inside these constructs never triggers
/// a runtime error or warning even when the accessed member doesn't
/// exist — that is the entire purpose of `isset()`/`empty()`.  Callers
/// use this to suppress unknown-member, unresolved-member, and
/// scalar-member-access diagnostics for spans that fall inside one of
/// these ranges.
pub(crate) fn compute_isset_empty_argument_ranges(content: &str) -> Vec<ByteRange> {
    crate::parser::with_parsed_program(content, "isset_empty_ranges", |program, _| {
        let mut ranges = Vec::new();
        crate::parser::for_each_node_with_ancestors(Node::Program(program), |node, _| {
            let parens = match node {
                Node::IssetConstruct(isset) => (isset.left_parenthesis, isset.right_parenthesis),
                Node::EmptyConstruct(empty) => (empty.left_parenthesis, empty.right_parenthesis),
                _ => return,
            };
            ranges.push((parens.0.end.offset as usize, parens.1.start.offset as usize));
        });
        ranges
    })
}

/// Information about symbols guarded by existence checks.
///
/// When code is wrapped in `if (function_exists('foo')) { foo(); }`, the
/// call to `foo()` should not produce an "unknown function" diagnostic
/// because the developer explicitly checked for its existence.
#[derive(Default)]
pub(crate) struct ExistenceGuards {
    /// Function names guarded by `function_exists('name')`.
    /// Maps function name (lowercase) to list of guarded byte ranges.
    pub function_guards: HashMap<String, Vec<ByteRange>>,
    /// Class names guarded by `class_exists(Name::class)` or `class_exists('Name')`.
    /// Maps class name (case-preserved) to list of guarded byte ranges.
    pub class_guards: HashMap<String, Vec<ByteRange>>,
    /// Method names guarded by `method_exists($obj, 'name')`.
    /// Maps method name (lowercase) to list of guarded byte ranges.
    pub method_guards: HashMap<String, Vec<ByteRange>>,
}

impl ExistenceGuards {
    /// Check whether a function call at `offset` is guarded by `function_exists()`.
    pub fn is_function_guarded(&self, name: &str, offset: u32) -> bool {
        self.function_guards
            .get(&name.to_lowercase())
            .is_some_and(|ranges| in_any(ranges, offset))
    }

    /// Check whether a class reference at `offset` is guarded by `class_exists()`.
    pub fn is_class_guarded(&self, name: &str, offset: u32) -> bool {
        // Check the name as-is first, then try the short name.
        self.class_guards
            .get(name)
            .or_else(|| {
                let short = name.rsplit('\\').next().unwrap_or(name);
                if short != name {
                    self.class_guards.get(short)
                } else {
                    None
                }
            })
            .is_some_and(|ranges| in_any(ranges, offset))
    }

    /// Check whether a member access at `offset` is guarded by `method_exists()`.
    pub fn is_method_guarded(&self, name: &str, offset: u32) -> bool {
        self.method_guards
            .get(&name.to_lowercase())
            .is_some_and(|ranges| in_any(ranges, offset))
    }
}

fn in_any(ranges: &[ByteRange], offset: u32) -> bool {
    let offset = offset as usize;
    ranges
        .iter()
        .any(|&(start, end)| offset >= start && offset < end)
}

/// Find the existence-check guards in `content` and the byte ranges they
/// protect.
///
/// Detects:
/// - `function_exists('name')` / `function_exists("name")`
/// - `class_exists(Name::class)` / `class_exists('Name')` / `class_exists("Name")`
/// - `method_exists($var, 'name')` / `method_exists($var, "name")`
///
/// A check guards the body of the `if` or `elseif` whose condition holds
/// it, and otherwise the rest of its own statement
/// (`function_exists('f') && f();`).  A negated check guards nothing,
/// since it typically surrounds a polyfill definition, unless its `if`
/// exits early (`if (!function_exists('f')) { return; }`): then it guards
/// the rest of the enclosing block.
pub(crate) fn compute_existence_guards(content: &str) -> ExistenceGuards {
    crate::parser::with_parsed_program(content, "existence_guards", |program, _| {
        let mut guards = ExistenceGuards::default();
        let file_end = content.len();
        crate::parser::for_each_node_with_ancestors(Node::Program(program), |node, ancestors| {
            let Node::FunctionCall(call) = node else {
                return;
            };
            let Some((kind, name)) = existence_check(call) else {
                return;
            };
            let Some(range) = guarded_range(call.span(), ancestors, file_end) else {
                return;
            };
            let (map, key) = match kind {
                ExistenceKind::Function => (&mut guards.function_guards, name.to_lowercase()),
                ExistenceKind::Class => (&mut guards.class_guards, name),
                ExistenceKind::Method => (&mut guards.method_guards, name.to_lowercase()),
            };
            map.entry(key).or_default().push(range);
        });
        guards
    })
}

/// The kind of existence check detected.
enum ExistenceKind {
    Function,
    Class,
    Method,
}

/// The kind of check `call` is and the name it checks, or `None` when it is
/// not an existence check naming a symbol literally.
fn existence_check(call: &FunctionCall<'_>) -> Option<(ExistenceKind, String)> {
    let Expression::Identifier(function) = call.function else {
        return None;
    };
    let function = bytes_to_str(function.value());
    let function = function.strip_prefix('\\').unwrap_or(function);
    let (kind, position) = if function.eq_ignore_ascii_case("function_exists") {
        (ExistenceKind::Function, 0)
    } else if function.eq_ignore_ascii_case("class_exists") {
        (ExistenceKind::Class, 0)
    } else if function.eq_ignore_ascii_case("method_exists") {
        (ExistenceKind::Method, 1)
    } else {
        return None;
    };
    let argument = call.argument_list.arguments.iter().nth(position)?.value();
    let name = match (&kind, argument) {
        (ExistenceKind::Class, _) if crate::parser::class_fetch_identifier(argument).is_some() => {
            bytes_to_str(crate::parser::class_fetch_identifier(argument)?.value()).to_string()
        }
        (_, Expression::Literal(Literal::String(string))) => {
            literal_bytes_to_str(string.value?)?.to_string()
        }
        _ => return None,
    };
    Some((kind, name))
}

/// The byte range a check at `call` guards, given the nodes enclosing it
/// (outermost first).
fn guarded_range(
    call: mago_span::Span,
    ancestors: &[Node<'_, '_>],
    file_end: usize,
) -> Option<ByteRange> {
    // The check is negated when what encloses it, past any parentheses,
    // is a `!`.
    let negated = ancestors
        .iter()
        .rev()
        .find(|node| !matches!(node, Node::Parenthesized(_) | Node::Expression(_) | Node::Call(_)))
        .is_some_and(|node| {
            matches!(node, Node::UnaryPrefix(prefix) if matches!(prefix.operator, UnaryPrefixOperator::Not(_)))
        });
    let within = |span: mago_span::Span| {
        span.start.offset <= call.start.offset && call.end.offset <= span.end.offset
    };

    for (depth, node) in ancestors.iter().enumerate().rev() {
        let (condition_open, condition, body_end, if_end, then_body) = match node {
            Node::If(if_stmt) => {
                let (body_end, then_body) = match &if_stmt.body {
                    IfBody::Statement(body) => (
                        body.statement.span().end.offset,
                        ThenBody::Statement(body.statement),
                    ),
                    IfBody::ColonDelimited(body) => (
                        body.statements
                            .last()
                            .map_or(body.colon.end.offset, |s| s.span().end.offset),
                        ThenBody::Statements(body.statements.as_slice()),
                    ),
                };
                (
                    if_stmt.left_parenthesis,
                    if_stmt.condition,
                    body_end,
                    if_stmt.span().end.offset,
                    then_body,
                )
            }
            Node::IfStatementBodyElseIfClause(clause) => (
                clause.left_parenthesis,
                clause.condition,
                clause.statement.span().end.offset,
                clause.statement.span().end.offset,
                ThenBody::Statement(clause.statement),
            ),
            Node::IfColonDelimitedBodyElseIfClause(clause) => (
                clause.left_parenthesis,
                clause.condition,
                clause
                    .statements
                    .last()
                    .map_or(clause.colon.end.offset, |s| s.span().end.offset),
                clause
                    .statements
                    .last()
                    .map_or(clause.colon.end.offset, |s| s.span().end.offset),
                ThenBody::Statements(clause.statements.as_slice()),
            ),
            _ => continue,
        };
        if !within(condition.span()) {
            continue;
        }
        if !negated {
            return Some((condition_open.start.offset as usize, body_end as usize));
        }
        if !then_body.exits_early() {
            return None;
        }
        // The rest of the block the `if` sits in.
        let scope_end = ancestors[..depth]
            .iter()
            .rev()
            .find_map(|node| match node {
                Node::Block(block) => Some(block.right_brace.start.offset as usize),
                _ => None,
            })
            .unwrap_or(file_end);
        return Some((if_end as usize, scope_end));
    }

    if negated {
        return None;
    }
    // No `if`: the rest of the check's own statement.
    let statement_end = ancestors.iter().rev().find_map(|node| match node {
        Node::Statement(statement) => Some(statement.span().end.offset),
        _ => None,
    })?;
    Some((call.end.offset as usize, statement_end as usize))
}

/// The branch an `if` takes when its condition holds.
enum ThenBody<'ast, 'arena> {
    Statement(&'ast Statement<'arena>),
    Statements(&'ast [Statement<'arena>]),
}

impl ThenBody<'_, '_> {
    /// Whether the branch leaves at once: its first statement returns,
    /// throws, exits, or jumps out of a loop.
    fn exits_early(&self) -> bool {
        let first = match self {
            ThenBody::Statement(Statement::Block(block)) => block.statements.first(),
            ThenBody::Statement(statement) => Some(*statement),
            ThenBody::Statements(statements) => statements.first(),
        };
        match first {
            Some(Statement::Return(_) | Statement::Continue(_) | Statement::Break(_)) => true,
            Some(Statement::Expression(statement)) => matches!(
                crate::parser::unwrap_parens(statement.expression),
                Expression::Throw(_)
                    | Expression::Construct(Construct::Exit(_) | Construct::Die(_))
            ),
            _ => false,
        }
    }
}
