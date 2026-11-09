//! Whether control can run off the end of a function body.
//!
//! [`statements_leave_block`](super::statements_leave_block) answers a
//! narrower question for guard clauses: whether every path through a list
//! leaves it.  Reaching the closing brace of a body takes more than that.
//! A `break` leaves its list but lands after the loop it targets, so a
//! `while (true)` loop only lets control out through a `break`; a `switch`
//! falls through from one case into the next; and a `try` is left by its
//! `finally` block whatever its body and catches did.
//!
//! Every `catch` is assumed reachable, since nearly any statement in the
//! `try` body can throw.

use mago_syntax::cst::*;

use super::exits::{contains_entry_label, expression_leaves};

/// The ways control leaves a statement.
#[derive(Clone, Copy, Default)]
struct Flow {
    /// Control can run off the end of the statement into the next one.
    normal: bool,
    /// The `break`s that leave the statement, one bit per level counted
    /// from the innermost loop or `switch` around it: bit 0 is a plain
    /// `break`, bit 1 a `break 2`.
    breaks: u64,
    /// The `continue`s that leave the statement, numbered the same way.
    continues: u64,
}

impl Flow {
    const NORMAL: Flow = Flow {
        normal: true,
        breaks: 0,
        continues: 0,
    };
    const GONE: Flow = Flow {
        normal: false,
        breaks: 0,
        continues: 0,
    };

    /// Add the jumps another path out of the same statement takes.
    fn join(self, other: Flow) -> Flow {
        Flow {
            normal: self.normal || other.normal,
            breaks: self.breaks | other.breaks,
            continues: self.continues | other.continues,
        }
    }

    /// The jumps that still leave once the loop or `switch` around this
    /// body has taken the ones aimed at it.
    fn escaping_loop(self) -> (u64, u64) {
        (self.breaks >> 1, self.continues >> 1)
    }
}

/// What only types can tell about the statements of a body.
///
/// [`FlowFacts::STRUCTURAL`] answers without types.  It can only ever find
/// more ways to the end of a body than there are, never fewer, so a body
/// it finds no way through needs no types at all.
pub(crate) struct FlowFacts<'a> {
    /// Whether an expression statement calls a `never`-returning function.
    pub never_call: &'a dyn Fn(&Expression<'_>) -> bool,
    /// Whether a `switch` without a `default` still has a case for every
    /// value its subject can hold.
    pub switch_is_exhaustive: &'a dyn Fn(&Switch<'_>) -> bool,
    /// Whether an `if` or `elseif` condition holds whenever control
    /// reaches it, so the chain cannot skip every branch.
    pub condition_always_holds: &'a dyn Fn(&Expression<'_>) -> bool,
}

impl FlowFacts<'static> {
    pub(crate) const STRUCTURAL: FlowFacts<'static> = FlowFacts {
        never_call: &|_| false,
        switch_is_exhaustive: &|_| false,
        condition_always_holds: &|_| false,
    };
}

/// Whether control can reach the end of `statements`, the body of a
/// function or method, without returning.
pub(crate) fn body_end_is_reachable(statements: &[Statement<'_>], facts: &FlowFacts<'_>) -> bool {
    list_flow(statements.iter(), facts).normal
}

/// The flow out of a statement list.
///
/// Statements after one that leaves cannot run, so their jumps do not
/// count, unless a `goto` label makes them reachable again (see
/// [`contains_entry_label`]).
fn list_flow<'s>(
    statements: impl Iterator<Item = &'s Statement<'s>>,
    facts: &FlowFacts<'_>,
) -> Flow {
    let mut flow = Flow::NORMAL;
    for statement in statements {
        if matches!(statement, Statement::Label(_)) {
            flow.normal = true;
        } else if flow.normal || contains_entry_label(statement) {
            let inner = statement_flow(statement, facts);
            flow.breaks |= inner.breaks;
            flow.continues |= inner.continues;
            flow.normal = inner.normal;
        }
    }
    flow
}

fn statement_flow(statement: &Statement<'_>, facts: &FlowFacts<'_>) -> Flow {
    match statement {
        Statement::Return(_) => Flow::GONE,
        // A `goto` either jumps back up, which makes this a loop control
        // only leaves by the other exits, or jumps forward to a label,
        // where the list picks control up again.
        Statement::Goto(_) => Flow::GONE,
        Statement::Break(r#break) => Flow {
            normal: false,
            breaks: level_bit(r#break.level),
            continues: 0,
        },
        Statement::Continue(r#continue) => Flow {
            normal: false,
            breaks: 0,
            continues: level_bit(r#continue.level),
        },
        Statement::Expression(statement) => {
            if expression_leaves(statement.expression, facts.never_call) {
                Flow::GONE
            } else {
                Flow::NORMAL
            }
        }
        Statement::Block(block) => list_flow(block.statements.iter(), facts),
        Statement::If(r#if) => if_flow(r#if, facts),
        Statement::Switch(switch) => switch_flow(switch, facts),
        Statement::While(r#while) => {
            let body = match &r#while.body {
                WhileBody::Statement(body) => statement_flow(body, facts),
                WhileBody::ColonDelimited(body) => list_flow(body.statements.iter(), facts),
            };
            loop_flow(body, is_literal_true(r#while.condition))
        }
        Statement::For(r#for) => {
            let body = match &r#for.body {
                ForBody::Statement(body) => statement_flow(body, facts),
                ForBody::ColonDelimited(body) => list_flow(body.statements.iter(), facts),
            };
            // With several conditions only the last one decides; with
            // none the loop runs until something leaves it.
            let forever = r#for
                .conditions
                .iter()
                .last()
                .is_none_or(|condition| is_literal_true(condition));
            loop_flow(body, forever)
        }
        Statement::DoWhile(do_while) => {
            let body = statement_flow(do_while.statement, facts);
            let (breaks, continues) = body.escaping_loop();
            // The condition is only tested by a body that runs to its end
            // or `continue`s.
            let tested = body.normal || body.continues & 1 != 0;
            Flow {
                normal: body.breaks & 1 != 0 || (tested && !is_literal_true(do_while.condition)),
                breaks,
                continues,
            }
        }
        Statement::Foreach(foreach) => {
            let body = match &foreach.body {
                ForeachBody::Statement(body) => statement_flow(body, facts),
                ForeachBody::ColonDelimited(body) => list_flow(body.statements.iter(), facts),
            };
            // An empty iterable skips the body entirely.
            loop_flow(body, false)
        }
        Statement::Try(r#try) => try_flow(r#try, facts),
        Statement::Declare(declare) => match &declare.body {
            DeclareBody::Statement(body) => statement_flow(body, facts),
            DeclareBody::ColonDelimited(body) => list_flow(body.statements.iter(), facts),
        },
        _ => Flow::NORMAL,
    }
}

fn if_flow(r#if: &If<'_>, facts: &FlowFacts<'_>) -> Flow {
    let (mut flow, has_else) = match &r#if.body {
        IfBody::Statement(body) => {
            let mut flow = statement_flow(body.statement, facts);
            for clause in body.else_if_clauses.iter() {
                flow = flow.join(statement_flow(clause.statement, facts));
            }
            if let Some(clause) = &body.else_clause {
                flow = flow.join(statement_flow(clause.statement, facts));
            }
            (flow, body.else_clause.is_some())
        }
        IfBody::ColonDelimited(body) => {
            let mut flow = list_flow(body.statements.iter(), facts);
            for clause in body.else_if_clauses.iter() {
                flow = flow.join(list_flow(clause.statements.iter(), facts));
            }
            if let Some(clause) = &body.else_clause {
                flow = flow.join(list_flow(clause.statements.iter(), facts));
            }
            (flow, body.else_clause.is_some())
        }
    };
    // Without an `else`, a chain whose conditions all fail skips every
    // branch, which cannot happen when one of the conditions always holds.
    // That only matters when no branch runs off its end, so the conditions
    // are not looked at otherwise.
    if !has_else && !flow.normal {
        let holds = |condition: &Expression<'_>| (facts.condition_always_holds)(condition);
        flow.normal = !(holds(r#if.condition)
            || match &r#if.body {
                IfBody::Statement(body) => body
                    .else_if_clauses
                    .iter()
                    .any(|clause| holds(clause.condition)),
                IfBody::ColonDelimited(body) => body
                    .else_if_clauses
                    .iter()
                    .any(|clause| holds(clause.condition)),
            });
    }
    flow
}

/// A `switch` is a loop as far as `break` and `continue` are concerned:
/// both leave it.  Each case is entered by its own match, so only the
/// last one decides whether control falls out of the bottom.
fn switch_flow(switch: &Switch<'_>, facts: &FlowFacts<'_>) -> Flow {
    let mut body = Flow::GONE;
    for case in switch.body.cases() {
        let case_flow = list_flow(case.statements().iter(), facts);
        body = Flow {
            normal: case_flow.normal,
            breaks: body.breaks | case_flow.breaks,
            continues: body.continues | case_flow.continues,
        };
    }
    let (breaks, continues) = body.escaping_loop();
    Flow {
        normal: body.normal
            || (body.breaks | body.continues) & 1 != 0
            || !(switch.body.has_default_case() || (facts.switch_is_exhaustive)(switch)),
        breaks,
        continues,
    }
}

/// A `while`, `for` or `foreach` loop with `body` as its body.  One that
/// does not run `forever` can end by its condition failing.
fn loop_flow(body: Flow, forever: bool) -> Flow {
    let (breaks, continues) = body.escaping_loop();
    Flow {
        normal: !forever || body.breaks & 1 != 0,
        breaks,
        continues,
    }
}

fn try_flow(r#try: &Try<'_>, facts: &FlowFacts<'_>) -> Flow {
    let mut flow = list_flow(r#try.block.statements.iter(), facts);
    for catch in r#try.catch_clauses.iter() {
        flow = flow.join(list_flow(catch.block.statements.iter(), facts));
    }
    let Some(finally) = &r#try.finally_clause else {
        return flow;
    };
    let finally = list_flow(finally.block.statements.iter(), facts);
    if finally.normal {
        // The block runs on every way out and then carries on along it.
        Flow {
            normal: flow.normal,
            breaks: flow.breaks | finally.breaks,
            continues: flow.continues | finally.continues,
        }
    } else {
        // Whatever the body and the catches did, the block leaves its own
        // way instead.
        finally
    }
}

/// The bit for a `break` or `continue` of `level`, which PHP requires to
/// be a positive integer literal when it is written at all.
fn level_bit(level: Option<&Expression<'_>>) -> u64 {
    let level = match level {
        None => 1,
        Some(Expression::Literal(Literal::Integer(integer))) => integer.value.unwrap_or(1),
        Some(Expression::Parenthesized(inner)) => return level_bit(Some(inner.expression)),
        Some(_) => 1,
    };
    1u64.checked_shl(level.saturating_sub(1).min(63) as u32)
        .unwrap_or(0)
}

/// Whether a loop condition is the literal `true`, the way an endless
/// loop is written.
fn is_literal_true(condition: &Expression<'_>) -> bool {
    match condition {
        Expression::Literal(Literal::True(_)) => true,
        Expression::Parenthesized(inner) => is_literal_true(inner.expression),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end_reachable(body: &str) -> bool {
        let source = format!("<?php function f() {{ {body} }}");
        crate::parser::with_parsed_program(&source, "fall_through_test", |program, _| {
            let Some(Statement::Function(function)) = program
                .statements
                .iter()
                .find(|s| matches!(s, Statement::Function(_)))
            else {
                panic!("no function parsed");
            };
            body_end_is_reachable(function.body.statements.as_slice(), &FlowFacts::STRUCTURAL)
        })
    }

    #[test]
    fn plain_exits_end_the_body() {
        assert!(!end_reachable("return 1;"));
        assert!(!end_reachable("throw new Exception();"));
        assert!(!end_reachable("exit(1);"));
        assert!(!end_reachable("die();"));
        assert!(end_reachable(""));
        assert!(end_reachable("foo();"));
    }

    #[test]
    fn an_if_needs_every_branch_and_an_else() {
        assert!(end_reachable("if ($x) { return 1; }"));
        assert!(end_reachable(
            "if ($x) { return 1; } elseif ($y) { return 2; }"
        ));
        assert!(!end_reachable(
            "if ($x) { return 1; } elseif ($y) { return 2; } else { throw new E(); }"
        ));
        assert!(end_reachable("if ($x) { return 1; } else { foo(); }"));
        assert!(!end_reachable("if ($x): return 1; else: return 2; endif;"));
    }

    #[test]
    fn an_endless_loop_ends_only_through_a_break() {
        assert!(!end_reachable("while (true) { foo(); }"));
        assert!(!end_reachable("for (;;) { if ($x) { return 1; } }"));
        assert!(!end_reachable("do { foo(); } while (true);"));
        assert!(end_reachable("while (true) { if ($x) { break; } }"));
        assert!(end_reachable("while ($x) { return 1; }"));
        assert!(end_reachable("foreach ($xs as $x) { return $x; }"));
        // A `continue` stays inside the loop.
        assert!(!end_reachable(
            "while (true) { if ($x) { continue; } return 1; }"
        ));
    }

    #[test]
    fn a_break_counts_levels_from_the_innermost_loop() {
        assert!(!end_reachable(
            "while (true) { foreach ($xs as $x) { break; } }"
        ));
        assert!(end_reachable(
            "while (true) { foreach ($xs as $x) { break 2; } }"
        ));
        assert!(end_reachable(
            "while (true) { switch ($x) { case 1: break 2; } }"
        ));
        // `continue` inside a `switch` acts on the `switch`.
        assert!(!end_reachable(
            "while (true) { switch ($x) { case 1: continue; } }"
        ));
    }

    #[test]
    fn a_do_while_tests_its_condition_after_the_body() {
        assert!(!end_reachable("do { return 1; } while ($x);"));
        assert!(end_reachable("do { foo(); } while ($x);"));
        assert!(end_reachable(
            "do { if ($y) { continue; } return 1; } while ($x);"
        ));
    }

    #[test]
    fn a_switch_without_default_can_match_nothing() {
        assert!(end_reachable("switch ($x) { case 1: return 1; }"));
        assert!(!end_reachable(
            "switch ($x) { case 1: return 1; default: return 2; }"
        ));
        // Falling through into a case that returns.
        assert!(!end_reachable(
            "switch ($x) { case 1: foo(); default: return 2; }"
        ));
        assert!(end_reachable(
            "switch ($x) { case 1: break; default: return 2; }"
        ));
        assert!(end_reachable(
            "switch ($x) { case 1: return 1; default: foo(); }"
        ));
    }

    #[test]
    fn a_try_needs_its_body_and_every_catch_to_leave() {
        assert!(!end_reachable(
            "try { return 1; } catch (E $e) { return 2; }"
        ));
        assert!(end_reachable("try { return 1; } catch (E $e) { log(); }"));
        assert!(!end_reachable("try { return 1; } finally { cleanup(); }"));
        assert!(!end_reachable(
            "try { foo(); } catch (E $e) { log(); } finally { return 3; }"
        ));
        assert!(!end_reachable(
            "try { return 1; } catch (A $e) { throw $e; } catch (B $e) { return 2; }"
        ));
    }

    #[test]
    fn a_match_leaves_when_every_arm_does() {
        assert!(!end_reachable(
            "match ($x) { 1 => throw new A(), default => throw new B() };"
        ));
        // No arm matching throws `UnhandledMatchError`.
        assert!(!end_reachable("match ($x) { 1 => throw new A() };"));
        assert!(end_reachable(
            "match ($x) { 1 => throw new A(), default => foo() };"
        ));
    }

    #[test]
    fn a_label_makes_the_code_after_it_reachable() {
        assert!(!end_reachable("start: foo(); goto start;"));
        assert!(end_reachable("goto end; return 1; end: foo();"));
        assert!(!end_reachable("goto end; foo(); end: return 1;"));
    }
}
