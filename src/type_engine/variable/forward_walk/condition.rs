//! What a condition proves for each of its outcomes, built from what its
//! operands prove.
//!
//! A condition is a tree of `&&`, `||` and `!` over leaves: comparisons,
//! `instanceof` checks, calls, bare values.  A leaf's narrowing comes from
//! the extractors in `cond_narrowing`, applied to the scope the leaf itself
//! was evaluated on.  The operators combine their operands' outcomes the
//! way PHP evaluates them:
//!
//! - `a && b`: `b` runs on `a`'s truthy scope, so the whole is truthy
//!   exactly when `b` is, and `b`'s truthy scope already carries what `a`
//!   proved.  It is falsey when either operand was.
//! - `a || b`: the mirror image.  `b` runs on `a`'s falsey scope.
//! - `!a`: `a`'s outcomes swapped.
//!
//! Composing per operand is what keeps a narrowing from an early operand
//! from being committed over a later operand's write to the same variable:
//! the truthy scope of `$x instanceof Foo && ($x = make())` is the one the
//! assignment left, not one `instanceof Foo` re-narrowed afterwards.
//!
//! Each outcome is worked out on first use and kept, since most consumers
//! need only one of them: the right operand of `&&` only ever reads its
//! left operand's truthy scope.

use std::cell::OnceCell;

use super::*;

use mago_syntax::cst::unary::UnaryPrefixOperator;

use crate::atom::atom;
use crate::parser::unwrap_parens;
use crate::type_engine::types::narrowing;

/// What evaluating a condition proved, for each of its outcomes.
pub(crate) struct Condition<'b> {
    node: Node<'b>,
    /// The condition wrote to the scope somewhere inside it.
    pub(crate) wrote: bool,
    truthy: OnceCell<Truthy>,
    falsey: OnceCell<ScopeState>,
}

/// A condition's truthy scope, with the subjects it pinned to a definite
/// class along the way.
struct Truthy {
    scope: ScopeState,
    /// See [`Condition::pinned`].
    pinned: Vec<String>,
}

enum Node<'b> {
    /// A condition the narrowing extractors read as a whole, with the
    /// scope its evaluation left behind.
    Leaf {
        expr: &'b Expression<'b>,
        after: ScopeState,
        /// The subjects the conjuncts before this one pinned (see
        /// `narrow_leaf_truthy`).
        conjoined: Vec<String>,
    },
    /// `!a`.
    Not(Box<Condition<'b>>),
    /// `a && b`, where `b` ran on `a`'s truthy scope.
    And(Box<Condition<'b>>, Box<Condition<'b>>),
    /// `a || b`, where `b` ran on `a`'s falsey scope.
    Or {
        left: Box<Condition<'b>>,
        right: Box<Condition<'b>>,
        /// The scope the disjunction started from.
        base: ScopeState,
        /// The subjects the conjuncts before this disjunction pinned, which
        /// keep the type `base` gives them (see [`Condition::or`]).
        pinned: Vec<String>,
    },
}

/// How a condition splits at its root.
pub(crate) enum Split<'b> {
    And(&'b Expression<'b>, &'b Expression<'b>),
    Or(&'b Expression<'b>, &'b Expression<'b>),
    /// A `!` over a logical chain.
    Not(&'b Expression<'b>),
    /// Anything else, with the parentheses, `(bool)` casts and pairs of
    /// `!` that change nothing about its outcomes taken off.
    Leaf(&'b Expression<'b>),
}

/// Split `expr` at its root operator.
///
/// A `!` over a single check stays a leaf: the extractors recognise the
/// negated spelling in place, and routing it through the opposite polarity
/// would change which commit path it takes.
pub(crate) fn split_condition<'b>(expr: &'b Expression<'b>) -> Split<'b> {
    // `!(!$x)` says exactly what `$x` says.
    let expr = unwrap_parens(narrowing::fold_negation_pairs(expr));
    if let Expression::UnaryPrefix(prefix) = expr
        && matches!(
            prefix.operator,
            UnaryPrefixOperator::BoolCast(..) | UnaryPrefixOperator::BooleanCast(..)
        )
    {
        return split_condition(prefix.operand);
    }
    if let Expression::Binary(bin) = expr {
        match bin.operator {
            BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => {
                return Split::And(bin.lhs, bin.rhs);
            }
            BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => {
                return Split::Or(bin.lhs, bin.rhs);
            }
            _ => {}
        }
    }
    match negated_logical_chain(expr) {
        Some(chain) => Split::Not(chain),
        None => Split::Leaf(expr),
    }
}

/// Narrow `scope` by `condition` without evaluating it: what holds once it
/// was found truthy, or falsey.
///
/// For the positions that read a condition but are not where it runs: an
/// `assert()` argument, a guard proved by a stored boolean, a ternary arm
/// read by the resolver.  The forward walker evaluates the conditions it
/// meets with [`process_condition`], which also applies their writes.
pub(crate) fn narrow_condition<'b>(
    condition: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Condition<'b> {
    narrow_condition_pinned(condition, scope, ctx, &[])
}

fn narrow_condition_pinned<'b>(
    condition: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    pinned: &[String],
) -> Condition<'b> {
    match split_condition(condition) {
        Split::Leaf(leaf) => Condition::leaf(leaf, scope.clone(), false, pinned),
        Split::Not(inner) => Condition::not(narrow_condition_pinned(inner, scope, ctx, &[])),
        Split::And(lhs, rhs) => {
            let left = narrow_condition_pinned(lhs, scope, ctx, pinned);
            let inner_pinned = with_pinned(pinned, left.pinned(ctx));
            let right = narrow_condition_pinned(rhs, left.truthy(ctx), ctx, &inner_pinned);
            Condition::and(left, right)
        }
        Split::Or(lhs, rhs) => {
            let mut base = scope.clone();
            seed_property_keys_into_scope(condition, &mut base, ctx);
            let left = narrow_condition_pinned(lhs, &base, ctx, pinned);
            let right = narrow_condition_pinned(rhs, left.falsey(ctx), ctx, pinned);
            Condition::or(left, right, base, pinned.to_vec())
        }
    }
}

/// `pinned` with `more` added.
pub(crate) fn with_pinned(pinned: &[String], more: &[String]) -> Vec<String> {
    let mut all = pinned.to_vec();
    for subject in more {
        if !all.contains(subject) {
            all.push(subject.clone());
        }
    }
    all
}

impl<'b> Condition<'b> {
    /// A condition the extractors read whole, evaluated to `after`, with
    /// the subjects the conjuncts before it pinned.
    pub(crate) fn leaf(
        expr: &'b Expression<'b>,
        after: ScopeState,
        wrote: bool,
        conjoined: &[String],
    ) -> Self {
        Self::new(
            Node::Leaf {
                expr,
                after,
                conjoined: conjoined.to_vec(),
            },
            wrote,
        )
    }

    /// `!inner`.
    pub(crate) fn not(inner: Condition<'b>) -> Self {
        let wrote = inner.wrote;
        Self::new(Node::Not(Box::new(inner)), wrote)
    }

    /// `left && right`, where `right` was evaluated on `left`'s truthy
    /// scope.
    pub(crate) fn and(left: Condition<'b>, right: Condition<'b>) -> Self {
        let wrote = left.wrote || right.wrote;
        Self::new(Node::And(Box::new(left), Box::new(right)), wrote)
    }

    /// `left || right`, where `right` was evaluated on `left`'s falsey
    /// scope and `base` is the scope the disjunction started from.
    ///
    /// `pinned` names the subjects a conjunct before the disjunction
    /// narrowed to a definite class.  Those keep the type `base` gives
    /// them: `$b instanceof Generic && ($cls === Generic::class || $b
    /// instanceof Template)` proves `$b` is a `Generic`, and a leg naming an
    /// unrelated class replaces rather than intersects, so joining the legs
    /// would answer `Generic|Template` and lose what the conjunct
    /// established.
    pub(crate) fn or(
        left: Condition<'b>,
        right: Condition<'b>,
        base: ScopeState,
        pinned: Vec<String>,
    ) -> Self {
        let wrote = left.wrote || right.wrote;
        Self::new(
            Node::Or {
                left: Box::new(left),
                right: Box::new(right),
                base,
                pinned,
            },
            wrote,
        )
    }

    fn new(node: Node<'b>, wrote: bool) -> Self {
        Self {
            node,
            wrote,
            truthy: OnceCell::new(),
            falsey: OnceCell::new(),
        }
    }

    /// The scope where the condition held.
    pub(crate) fn truthy(&self, ctx: &ForwardWalkCtx<'_>) -> &ScopeState {
        &self.truthy_outcome(ctx).scope
    }

    /// The scope where the condition failed.
    pub(crate) fn falsey(&self, ctx: &ForwardWalkCtx<'_>) -> &ScopeState {
        self.falsey.get_or_init(|| match &self.node {
            Node::Leaf { expr, after, .. } => {
                let mut scope = after.clone();
                narrow_leaf_falsey(expr, &mut scope, ctx);
                scope
            }
            Node::Not(inner) => inner.truthy(ctx).clone(),
            Node::And(left, right) => join_outcomes(left.falsey(ctx), right.falsey(ctx)),
            Node::Or { right, .. } => right.falsey(ctx).clone(),
        })
    }

    /// The subjects the truthy outcome narrowed to a definite class by a
    /// positive `instanceof`-style check, which a disjunction further
    /// along an `&&` chain must not widen back (see [`Self::or`]).
    pub(crate) fn pinned(&self, ctx: &ForwardWalkCtx<'_>) -> &[String] {
        &self.truthy_outcome(ctx).pinned
    }

    fn truthy_outcome(&self, ctx: &ForwardWalkCtx<'_>) -> &Truthy {
        self.truthy.get_or_init(|| match &self.node {
            Node::Leaf {
                expr,
                after,
                conjoined,
            } => {
                let mut scope = after.clone();
                let pinned = narrow_leaf_truthy(expr, &mut scope, ctx, conjoined);
                Truthy { scope, pinned }
            }
            Node::Not(inner) => Truthy {
                scope: inner.falsey(ctx).clone(),
                pinned: Vec::new(),
            },
            Node::And(left, right) => Truthy {
                scope: right.truthy(ctx).clone(),
                pinned: with_pinned(left.pinned(ctx), right.pinned(ctx)),
            },
            Node::Or {
                left,
                right,
                base,
                pinned,
            } => Truthy {
                scope: join_legs(left, right, base, pinned, ctx),
                pinned: Vec::new(),
            },
        })
    }

    /// The scope a leg's narrowing started from, which is what a leg's
    /// answer is checked against when the legs of a disjunction are joined.
    fn narrowing_base(&self) -> &ScopeState {
        match &self.node {
            Node::Leaf { after, .. } => after,
            Node::Not(inner) => inner.narrowing_base(),
            Node::And(left, _) => left.narrowing_base(),
            Node::Or { left, .. } => left.narrowing_base(),
        }
    }
}

/// The scope where either of two outcomes happened.
///
/// A synthetic key only one of them established is not a fact about the
/// join: the other says nothing about it, so the declared type still
/// stands.  Without this, `!($n === 'self' && $s->isInClass())` left
/// `$s->getClassReflection()` narrowed to the `null` that one alternative
/// implies, and a sibling `elseif` that proves the opposite could no longer
/// widen it back.
fn join_outcomes(a: &ScopeState, b: &ScopeState) -> ScopeState {
    let mut joined = a.clone();
    joined.merge_branch(b);
    retain_synthetic_keys_common_to_all(&mut joined, &[a, b]);
    joined
}

/// The truthy scope of `left || right`: the join of the scopes each leg
/// held in.
///
/// Going through the join is what records which leg proved what, so a
/// check further down that rules the other leg out recovers the surviving
/// leg's conclusions:
///
/// ```php
/// if ($n->keyVar === null || ($n->keyVar instanceof Variable && is_string($n->keyVar->name))) {
///     $name = $n->keyVar instanceof Variable ? $n->keyVar->name : null;   // string|null
/// }
/// ```
fn join_legs(
    left: &Condition<'_>,
    right: &Condition<'_>,
    base: &ScopeState,
    pinned: &[String],
    ctx: &ForwardWalkCtx<'_>,
) -> ScopeState {
    let mut legs = [left.truthy(ctx).clone(), right.truthy(ctx).clone()];
    drop_leg_answers_the_base_rules_out(&mut legs[0], left.narrowing_base());
    drop_leg_answers_the_base_rules_out(&mut legs[1], right.narrowing_base());
    let [first, second] = &legs;
    let mut joined = join_outcomes(first, second);
    for subject in pinned {
        let key = atom(subject);
        if let Some(types) = base.locals.get(&key) {
            joined.locals.insert(key, types.clone());
        }
    }
    joined
}
