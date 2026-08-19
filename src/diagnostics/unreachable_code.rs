//! Unreachable code diagnostics.
//!
//! Statements that follow one which always leaves the block cannot run:
//!
//! ```php
//! function f(): void {
//!     return;
//!     echo 'never';   // dimmed
//! }
//! ```
//!
//! Reported as a `Hint` carrying [`DiagnosticTag::UNNECESSARY`], so editors
//! grey the text out rather than underlining it.  Dead code is tidying, not
//! a defect, and it is rendered the way unused imports already are.
//!
//! This is a Phase 1 check: it reads the shape of the statement list and
//! nothing else, so it needs no type resolution and runs on every keystroke
//! alongside the syntax and unused-symbol checks.
//!
//! ## What counts as leaving the block
//!
//! `return`, `throw`, `exit`, `die`, `continue`, `break`, and `goto`, plus a
//! block or an `if` whose every branch does one of those — an `if` without an
//! `else` always has a path that falls through, so it never qualifies.
//!
//! A call to a function declared `never` leaves too, and is deliberately not
//! recognised here: knowing that takes the type engine, which would move this
//! check into the expensive phase to catch a case the reader can already see.
//! [`crate::type_engine`] has its own answer to this question for narrowing,
//! where the types are already in hand.
//!
//! ## What is not dead
//!
//! A declaration at the top level of a file is hoisted — PHP binds it before
//! the script runs, so it exists whether or not control reaches the line it is
//! written on.  The same declaration nested inside a function body is not
//! hoisted: it is created by executing that statement, so after a `return` it
//! really is dead.  The two cases are told apart by where they sit, not by
//! what kind of statement they are.  Imports and the tags around them declare
//! nothing at runtime and hold wherever they sit.
//!
//! A `goto` label is an entry point.  Code after one is reachable by jumping
//! to it, so a label ends the dead run rather than being swallowed by it —
//! whether any `goto` actually targets it is not something this check tries to
//! establish.  For the same reason a `goto` ends a run only inside the list it
//! is written in: a jump to a label in the same block leaves the block running,
//! and without resolving the label the two cannot be told apart.
//!
//! Since both of those can sit in the middle of otherwise dead code, one block
//! can hold several separate dead runs, and each is reported on its own.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::parser::with_parsed_program;

use super::helpers::make_diagnostic;

/// Diagnostic code for code that cannot be reached.
pub(crate) const UNREACHABLE_CODE_CODE: &str = "unreachable_code";

/// A run of statements that cannot be reached, as byte offsets.
struct DeadRun {
    start: u32,
    end: u32,
}

impl Backend {
    /// Collect unreachable-code diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them.
    pub fn collect_unreachable_code_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let runs = with_parsed_program(content, "unreachable_code", |program, _content| {
            let mut runs = Vec::new();
            collect_from_program(program, &mut runs);
            collect_from_function_expressions(program, &mut runs);
            runs
        });

        for run in runs {
            let Some(range) =
                self.offset_range_to_lsp_range(uri, content, run.start as usize, run.end as usize)
            else {
                continue;
            };
            let mut diagnostic = make_diagnostic(
                range,
                DiagnosticSeverity::HINT,
                UNREACHABLE_CODE_CODE,
                "Unreachable code".to_string(),
            );
            diagnostic.tags = Some(vec![DiagnosticTag::UNNECESSARY]);
            out.push(diagnostic);
        }
    }
}

/// Walk a file's top-level statements.
///
/// A `return` here ends the script, so the top level has dead runs like any
/// block — it differs only in that its declarations are hoisted.
fn collect_from_program(program: &Program<'_>, runs: &mut Vec<DeadRun>) {
    scan_statements(program.statements.iter(), Scope::TopLevel, runs);
}

/// Walk the bodies that hang off expressions rather than statements.
///
/// A closure or an anonymous class is written inside an expression, so the
/// statement recursion above never reaches it — and a callback with dead code
/// in it is ordinary PHP.  The walker finds them wherever they are nested,
/// including inside each other, and the two passes cannot overlap because the
/// statement recursion never descends into an expression.
fn collect_from_function_expressions(program: &Program<'_>, runs: &mut Vec<DeadRun>) {
    let collector = FunctionExpressionBodies;
    mago_syntax::walker::Walker::walk_program(&collector, program, runs);
}

struct FunctionExpressionBodies;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<DeadRun>>
    for FunctionExpressionBodies
{
    fn walk_in_closure(&self, closure: &'ast Closure<'arena>, runs: &mut Vec<DeadRun>) {
        scan_block(&closure.body, runs);
    }

    fn walk_in_anonymous_class(
        &self,
        class: &'ast AnonymousClass<'arena>,
        runs: &mut Vec<DeadRun>,
    ) {
        scan_members(&class.members, runs);
    }
}

/// Whether a statement list is the file's top level, which decides only one
/// thing: whether a declaration in it is hoisted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    TopLevel,
    Nested,
}

/// Scan one statement list for dead runs, then descend into every statement
/// it holds.
fn scan_statements<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
    scope: Scope,
    runs: &mut Vec<DeadRun>,
) {
    let statements: Vec<&Statement<'_>> = statements.collect();

    let mut dead_from: Option<usize> = None;
    for (index, statement) in statements.iter().enumerate() {
        match dead_from {
            // Already past a statement that left the block.  A label makes
            // what follows reachable again, so it closes the run it
            // interrupts rather than joining it.
            Some(start) => {
                if matches!(statement, Statement::Label(_)) {
                    push_run(&statements[start..index], scope, runs);
                    dead_from = None;
                }
            }
            None => {
                if ends_run(statement) && index + 1 < statements.len() {
                    dead_from = Some(index + 1);
                }
            }
        }
    }
    if let Some(start) = dead_from {
        push_run(&statements[start..], scope, runs);
    }

    for statement in statements {
        descend(statement, runs);
    }
}

/// Record one contiguous dead run, minus anything in it that is not dead.
///
/// A hoisted declaration inside the run keeps its own meaning, so the run is
/// split around it instead of covering it — dimming a class that PHP has
/// already bound would be a lie about what the code does.
fn push_run(statements: &[&Statement<'_>], scope: Scope, runs: &mut Vec<DeadRun>) {
    let mut segment_start: Option<u32> = None;
    let mut segment_end: u32 = 0;

    for statement in statements {
        if is_hoisted(statement, scope) {
            if let Some(start) = segment_start.take() {
                runs.push(DeadRun {
                    start,
                    end: segment_end,
                });
            }
            continue;
        }
        let span = statement.span();
        if segment_start.is_none() {
            segment_start = Some(span.start.offset);
        }
        segment_end = span.end.offset;
    }

    if let Some(start) = segment_start {
        runs.push(DeadRun {
            start,
            end: segment_end,
        });
    }
}

/// Whether a statement is bound before the code around it runs, and so keeps
/// its meaning wherever it is written.
///
/// Imports and the tags around them declare nothing at runtime and hold
/// wherever they sit.  A `function` or `class` is hoisted only at the top
/// level of a file: nested inside a function body it is created by executing
/// the statement, so after a `return` it never comes into being at all.
fn is_hoisted(statement: &Statement<'_>, scope: Scope) -> bool {
    if matches!(
        statement,
        Statement::Use(_)
            | Statement::Namespace(_)
            | Statement::Declare(_)
            | Statement::OpeningTag(_)
            | Statement::ClosingTag(_)
            | Statement::Noop(_)
    ) {
        return true;
    }
    scope == Scope::TopLevel
        && matches!(
            statement,
            Statement::Function(_)
                | Statement::Class(_)
                | Statement::Interface(_)
                | Statement::Trait(_)
                | Statement::Enum(_)
                | Statement::Constant(_)
        )
}

/// Whether this statement ends the run of reachable code in its own list.
///
/// That is [`leaves_block`] plus `goto`, which hands control to its label.
/// `goto` counts only here, never when the result is propagated outwards by
/// [`leaves_block`]: a jump whose label sits in the same block leaves the
/// block running perfectly well, and without resolving the label there is no
/// way to tell which kind it is.  Treating it as an exit out there would dim
/// live code, so it is treated as one only where a label can end the run.
fn ends_run(statement: &Statement<'_>) -> bool {
    matches!(statement, Statement::Goto(_)) || leaves_block(statement)
}

/// Whether every path through this statement leaves the enclosing block.
///
/// Mirrors the syntactic half of
/// [`crate::type_engine`]'s `statement_unconditionally_exits`, which answers
/// the same question with types in hand so it can also recognise a call to a
/// `never`-returning function.  Sharing one implementation would drag the
/// resolver into a check that deliberately runs without it.
fn leaves_block(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::Return(_) | Statement::Continue(_) | Statement::Break(_) => true,
        // `throw`, `exit`, and `die` are expressions in PHP, so they reach
        // here wrapped in an expression statement.
        Statement::Expression(expression) => {
            matches!(
                expression.expression,
                Expression::Throw(_)
                    | Expression::Construct(Construct::Exit(_))
                    | Expression::Construct(Construct::Die(_))
            )
        }
        Statement::Block(block) => list_leaves_block(block.statements.iter()),
        Statement::If(if_statement) => if_leaves_block(&if_statement.body),
        _ => false,
    }
}

/// Whether an `if` leaves the block down every branch.
///
/// Needs the then-branch, every `elseif`, and an `else` that exists — without
/// the `else` there is a path that runs none of them and falls through.
fn if_leaves_block(body: &IfBody<'_>) -> bool {
    match body {
        IfBody::Statement(body) => {
            leaves_block(body.statement)
                && body
                    .else_if_clauses
                    .iter()
                    .all(|clause| leaves_block(clause.statement))
                && body
                    .else_clause
                    .as_ref()
                    .is_some_and(|clause| leaves_block(clause.statement))
        }
        IfBody::ColonDelimited(body) => {
            list_leaves_block(body.statements.iter())
                && body
                    .else_if_clauses
                    .iter()
                    .all(|clause| list_leaves_block(clause.statements.iter()))
                && body
                    .else_clause
                    .as_ref()
                    .is_some_and(|clause| list_leaves_block(clause.statements.iter()))
        }
    }
}

/// Whether control is gone by the end of a statement list.
///
/// Not the same question as "does anything in here leave": a `goto` label
/// after the exit is an entry point, so control can be back in the list and
/// run out of its end.  The list has to be walked for that to show, and a
/// list that ends reachable leaves whatever encloses it running — which is
/// why asking `any` here dims the live code after a block whose exit was
/// jumped over.
///
/// A `goto` itself does not end anything here.  It ends the run it sits in
/// (see [`ends_run`]), but where its label lives is unknown, so counting it
/// as an exit would claim the enclosing block never falls through when the
/// jump may simply land further down the same list.
fn list_leaves_block<'a>(statements: impl Iterator<Item = &'a Statement<'a>>) -> bool {
    let mut reachable = true;
    for statement in statements {
        if matches!(statement, Statement::Label(_)) {
            reachable = true;
        } else if reachable && leaves_block(statement) {
            reachable = false;
        }
    }
    !reachable
}

/// Descend into every statement list a statement contains.
///
/// Each body is scanned in its own right: a `return` inside one loop says
/// nothing about the statements after that loop.
fn descend(statement: &Statement<'_>, runs: &mut Vec<DeadRun>) {
    match statement {
        Statement::Block(block) => scan_block(block, runs),
        Statement::Namespace(namespace) => {
            // A namespace declaration does not run, so what it holds is still
            // the top level as far as hoisting is concerned.
            scan_statements(namespace.statements().iter(), Scope::TopLevel, runs);
        }
        Statement::Function(function) => scan_block(&function.body, runs),
        Statement::Class(class) => scan_members(&class.members, runs),
        Statement::Interface(interface) => scan_members(&interface.members, runs),
        Statement::Trait(r#trait) => scan_members(&r#trait.members, runs),
        Statement::Enum(r#enum) => scan_members(&r#enum.members, runs),
        Statement::If(if_statement) => descend_if(if_statement, runs),
        Statement::Try(r#try) => {
            scan_block(&r#try.block, runs);
            for catch in r#try.catch_clauses.iter() {
                scan_block(&catch.block, runs);
            }
            if let Some(finally) = &r#try.finally_clause {
                scan_block(&finally.block, runs);
            }
        }
        Statement::Foreach(foreach) => match &foreach.body {
            ForeachBody::Statement(statement) => descend(statement, runs),
            ForeachBody::ColonDelimited(body) => {
                scan_statements(body.statements.iter(), Scope::Nested, runs)
            }
        },
        Statement::For(r#for) => match &r#for.body {
            ForBody::Statement(statement) => descend(statement, runs),
            ForBody::ColonDelimited(body) => {
                scan_statements(body.statements.iter(), Scope::Nested, runs)
            }
        },
        Statement::While(r#while) => match &r#while.body {
            WhileBody::Statement(statement) => descend(statement, runs),
            WhileBody::ColonDelimited(body) => {
                scan_statements(body.statements.iter(), Scope::Nested, runs)
            }
        },
        Statement::DoWhile(do_while) => descend(do_while.statement, runs),
        Statement::Switch(switch) => {
            for case in switch.body.cases() {
                scan_statements(case.statements().iter(), Scope::Nested, runs);
            }
        }
        Statement::Declare(declare) => match &declare.body {
            DeclareBody::Statement(statement) => descend(statement, runs),
            DeclareBody::ColonDelimited(body) => {
                scan_statements(body.statements.iter(), Scope::Nested, runs)
            }
        },
        _ => {}
    }
}

fn scan_block(block: &Block<'_>, runs: &mut Vec<DeadRun>) {
    scan_statements(block.statements.iter(), Scope::Nested, runs);
}

/// Walk the bodies of a class-like declaration's methods.
fn scan_members(members: &Sequence<'_, ClassLikeMember<'_>>, runs: &mut Vec<DeadRun>) {
    for member in members.iter() {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            scan_block(block, runs);
        }
    }
}

fn descend_if(if_statement: &If<'_>, runs: &mut Vec<DeadRun>) {
    match &if_statement.body {
        IfBody::Statement(body) => {
            descend(body.statement, runs);
            for clause in body.else_if_clauses.iter() {
                descend(clause.statement, runs);
            }
            if let Some(clause) = &body.else_clause {
                descend(clause.statement, runs);
            }
        }
        IfBody::ColonDelimited(body) => {
            scan_statements(body.statements.iter(), Scope::Nested, runs);
            for clause in body.else_if_clauses.iter() {
                scan_statements(clause.statements.iter(), Scope::Nested, runs);
            }
            if let Some(clause) = &body.else_clause {
                scan_statements(clause.statements.iter(), Scope::Nested, runs);
            }
        }
    }
}
