//! Branches the source alone rules out.
//!
//! A guard whose value is decidable from the source decides its branches
//! too: `if (false) { … }` never runs, and neither does the negated arm of
//! a `method_exists()` check on a class and method both spelled out. Two
//! consumers care.
//!
//! The walker marks such a branch's scope unreachable so the join after
//! the `if` ignores whatever the branch would have established.
//!
//! Diagnostics care about a stronger thing: nothing inside a branch that
//! cannot run should be judged at all. Every collector walks spans of its
//! own and has no idea which of them the control flow can reach, so the
//! ranges are collected here while a pass is running and applied once to
//! the pass's output.

use std::sync::Arc;

use super::*;
use crate::atom::{Atom, bytes_to_str};
use crate::type_engine::types::narrowing::find_method_in_chain_where;
use crate::type_engine::types::narrowing::{argument_value, string_literal_value};
use crate::types::ClassInfo;

thread_local! {
    /// Byte ranges of the branches a decidable guard rules out in the
    /// file the running diagnostic pass is walking.  `None` outside such
    /// a pass, which is when nothing records and nothing reads.
    static UNREACHABLE_RANGES: RefCell<Option<Vec<(u32, u32)>>> = const { RefCell::new(None) };
}

/// Start collecting the ranges the walk proves unreachable.
pub(crate) fn begin_unreachable_collection() {
    UNREACHABLE_RANGES.with(|cell| *cell.borrow_mut() = Some(Vec::new()));
}

/// Stop collecting and discard what was collected.
pub(crate) fn end_unreachable_collection() {
    UNREACHABLE_RANGES.with(|cell| *cell.borrow_mut() = None);
}

/// Record a byte range the walk proved cannot run.
///
/// A no-op when no collection is in progress, so the walker can call it
/// unconditionally.
pub(crate) fn record_unreachable_range(range: (u32, u32)) {
    UNREACHABLE_RANGES.with(|cell| {
        if let Some(ranges) = cell.borrow_mut().as_mut()
            && !ranges.contains(&range)
        {
            ranges.push(range);
        }
    });
}

/// Record the span a sequence of statements covers as unreachable.
///
/// The alternative-syntax branch bodies (`if (…): … endif;`) are lists of
/// statements rather than one block, so their extent is the first
/// statement's start to the last one's end.
pub(crate) fn record_statements_unreachable<'b>(
    mut statements: impl Iterator<Item = &'b Statement<'b>>,
) {
    let Some(first) = statements.next() else {
        return;
    };
    let start = first.span().start.offset;
    let end = statements
        .last()
        .map_or(first.span().end.offset, |last| last.span().end.offset);
    record_unreachable_range((start, end));
}

/// The ranges recorded so far, or an empty vector when nothing is
/// collecting.
pub(crate) fn unreachable_ranges() -> Vec<(u32, u32)> {
    UNREACHABLE_RANGES.with(|cell| cell.borrow().clone().unwrap_or_default())
}

/// Which branches of an `if` chain a decidable guard rules out.
///
/// The flags in `else_if_clauses` are index-aligned with the chain's
/// `elseif` clauses.
pub(crate) struct DeadIfBranches {
    pub then_branch: bool,
    pub else_if_clauses: Vec<bool>,
    pub else_clause: bool,
}

impl DeadIfBranches {
    /// Whether any branch of the chain is ruled out.
    pub(crate) fn any(&self) -> bool {
        self.then_branch || self.else_clause || self.else_if_clauses.iter().any(|dead| *dead)
    }
}

/// Fold an `if` chain's conditions and report which of its branches
/// cannot run.
///
/// A condition that folds to `false` rules out its own branch; one that
/// folds to `true` rules out everything after it, because PHP takes the
/// first branch whose condition holds.
pub(crate) fn dead_if_branches<'b>(
    condition: &'b Expression<'b>,
    else_if_conditions: impl Iterator<Item = &'b Expression<'b>>,
    has_else_clause: bool,
    ctx: &ForwardWalkCtx<'_>,
) -> DeadIfBranches {
    let leading = constant_condition_value(condition, ctx);
    // Whether an earlier condition in the chain is known to hold, which
    // is what makes every later branch dead.
    let mut chain_taken = leading == Some(true);

    let mut else_if_clauses = Vec::new();
    for cond in else_if_conditions {
        let value = constant_condition_value(cond, ctx);
        else_if_clauses.push(chain_taken || value == Some(false));
        chain_taken |= value == Some(true);
    }

    DeadIfBranches {
        then_branch: leading == Some(false),
        else_if_clauses,
        else_clause: has_else_clause && chain_taken,
    }
}

/// Fold a condition to the value the source alone decides it has.
///
/// `None` for everything whose value depends on runtime state, which is
/// almost every real condition.  The decidable shapes are the boolean
/// literals, the logical operators over decidable operands, and
/// [`method_exists`](method_exists_value).
pub(crate) fn constant_condition_value(
    condition: &Expression<'_>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<bool> {
    match condition {
        Expression::Parenthesized(inner) => constant_condition_value(inner.expression, ctx),
        Expression::Literal(Literal::True(_)) => Some(true),
        Expression::Literal(Literal::False(_)) => Some(false),
        // A bare `true`/`false` reaches the parser as a constant name
        // when written with a leading `\` or in unusual case.
        Expression::ConstantAccess(access) => {
            let name = crate::util::strip_fqn_prefix(bytes_to_str(access.name.value()));
            if name.eq_ignore_ascii_case("true") {
                Some(true)
            } else if name.eq_ignore_ascii_case("false") {
                Some(false)
            } else {
                None
            }
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            constant_condition_value(prefix.operand, ctx).map(|value| !value)
        }
        // The operator is matched before either operand is folded: a
        // comparison is by far the most common condition shape and
        // nothing about it is decidable, so it must cost one match.
        Expression::Binary(binary) => match binary.operator {
            BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => {
                // A `false` operand decides the conjunction whichever
                // side it is on: an undecidable left operand either
                // short-circuits (false) or lets the `false` through.
                match (
                    constant_condition_value(binary.lhs, ctx),
                    constant_condition_value(binary.rhs, ctx),
                ) {
                    (Some(false), _) | (_, Some(false)) => Some(false),
                    (Some(true), Some(true)) => Some(true),
                    _ => None,
                }
            }
            BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => match (
                constant_condition_value(binary.lhs, ctx),
                constant_condition_value(binary.rhs, ctx),
            ) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            },
            _ => None,
        },
        Expression::Call(Call::Function(_)) => method_exists_value(condition, ctx),
        _ => None,
    }
}

/// Fold `method_exists()` over arguments that name one class and one
/// method.
///
/// Answers `Some(true)` when the method is found on the named class or
/// anywhere in its chain.  Not finding it is not proof of absence — the
/// class may be one nothing has loaded, or carry the method through a
/// provider this lookup does not consult — so the absent case answers
/// `None` rather than `Some(false)`.
fn method_exists_value(expr: &Expression<'_>, ctx: &ForwardWalkCtx<'_>) -> Option<bool> {
    let Expression::Call(Call::Function(call)) = expr else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    if !bytes_to_str(ident.value())
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("method_exists")
    {
        return None;
    }
    let args: Vec<_> = call.argument_list.arguments.iter().collect();
    if args.len() != 2 {
        return None;
    }
    let subject = static_class_of(argument_value(args[0]), ctx)?;
    let method = string_literal_value(argument_value(args[1]))?;

    // The chain is walked with raw class loads rather than a full
    // resolve: this runs inside the forward walker, where the enclosing
    // class may itself be mid-resolution and a merge would write a
    // partial result into the shared resolved-class cache.
    let mut visited: Vec<Atom> = Vec::new();
    find_method_in_chain_where(
        &subject,
        &method,
        ctx.class_loader,
        &|_| true,
        &mut visited,
        0,
    )
    .map(|_| true)
}

/// The class an expression names, when the source pins it to exactly one.
///
/// Covers `Foo::class`, `self::class`, `static::class`, `parent::class`
/// and a class name written as a string literal.  `static::class` may
/// name a subclass at runtime, which only ever adds methods, so the
/// declaring class answers for it.
fn static_class_of(expr: &Expression<'_>, ctx: &ForwardWalkCtx<'_>) -> Option<Arc<ClassInfo>> {
    match expr {
        Expression::Parenthesized(inner) => static_class_of(inner.expression, ctx),
        Expression::Access(Access::ClassConstant(access)) => {
            let ClassLikeConstantSelector::Identifier(constant) = &access.constant else {
                return None;
            };
            if !bytes_to_str(constant.value).eq_ignore_ascii_case("class") {
                return None;
            }
            match access.class {
                Expression::Self_(_) | Expression::Static(_) => {
                    (ctx.class_loader)(&ctx.current_class.name)
                }
                Expression::Parent(_) => {
                    let parent = ctx.current_class.parent_class.as_ref()?;
                    (ctx.class_loader)(parent)
                }
                Expression::Identifier(ident) => {
                    let name = bytes_to_str(ident.value());
                    let fqn = crate::util::resolve_name_via_loader(name, ctx.class_loader);
                    (ctx.class_loader)(&fqn).or_else(|| (ctx.class_loader)(name))
                }
                _ => None,
            }
        }
        _ => string_literal_value(expr).and_then(|name| (ctx.class_loader)(&name)),
    }
}
