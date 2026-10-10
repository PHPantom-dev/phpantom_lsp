//! One pass over an expression, in the order PHP evaluates it.
//!
//! PHP assignments are expressions, so a write can sit anywhere a value
//! can: a condition (`if ($x = expr())`), a call receiver
//! (`($x = $map[$key])->truthy()`), an argument
//! (`is_object($token = $tokenizer->next())`), an array item
//! (`[$d++, $d]`).  Whatever is evaluated after the write reads what it
//! wrote, and whatever was evaluated before it read the value it replaced.
//! [`process_expr`] walks the expression once, threading the scope from
//! each sub-expression to the next, so each one sees the scope at its own
//! position:
//!
//! - Each write is applied where it happens and a snapshot is recorded at
//!   its end, so a diagnostic lookup further along the expression finds
//!   the written type.
//! - The right operand of `&&` / `||` runs on the scope the left one left
//!   behind, narrowed by what the left one proved (or its inverse, for
//!   `||`), and the snapshot at its start is recorded from that scope.
//!   The right operand of `??` and the arms of a ternary or `match` run
//!   on their own copy, joined with the path where they did not run.
//! - The expression's own type is still resolved once, by the caller,
//!   against the scope after the whole expression.  That answer is wrong
//!   for a sub-expression evaluated before a later write and for the old
//!   value `$d++` yields, so those are typed where they ran and recorded
//!   in [`ExprTypes`], which the resolver reads back.

use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::unary::{UnaryPostfixOperator, UnaryPrefixOperator};

use crate::atom::bytes_to_str;
use crate::types::ResolvedType;

/// The types of sub-expressions recorded where they were evaluated, keyed
/// by span.  See [`VarResolutionCtx::expr_types`](crate::type_engine::resolver::VarResolutionCtx::expr_types).
///
/// Only an expression that writes partway through records anything, and
/// then only a handful of nodes, so a list beats a map.
#[derive(Default)]
pub(crate) struct ExprTypes(Vec<(u32, u32, Vec<ResolvedType>)>);

impl ExprTypes {
    /// The types recorded for `expr`, if any.
    pub(crate) fn get(&self, expr: &Expression<'_>) -> Option<&[ResolvedType]> {
        if self.0.is_empty() {
            return None;
        }
        let span = expr.span();
        self.0
            .iter()
            .find(|(start, end, _)| *start == span.start.offset && *end == span.end.offset)
            .map(|(_, _, types)| types.as_slice())
    }

    /// Record `types` for `expr`.  A node that resolved to nothing is left
    /// out, so the resolver works it out the ordinary way instead of
    /// reading a recorded blank.
    fn insert(&mut self, expr: &Expression<'_>, types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        let span = expr.span();
        self.0.push((span.start.offset, span.end.offset, types));
    }
}

/// What processing one expression did.
#[derive(Clone, Copy, Default)]
pub(crate) struct ExprResult {
    /// The expression wrote to the scope somewhere inside it.
    pub(crate) wrote: bool,
}

/// Walk `expr` in evaluation order, applying the writes it makes to
/// `scope` and recording the snapshots described in the module docs.
///
/// Returns the sub-expression types the caller should resolve the
/// expression's own value with (see [`resolve_rhs_with_types`]).
///
/// The outermost assignment of a *statement* is not this function's job:
/// `process_assignment_expr` owns that one, and knows about destructuring,
/// `@var` overrides, and indexed writes that this walk does not.
pub(crate) fn process_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> ExprTypes {
    let mut walk = ExprWalk::default();
    walk.expr(expr, scope, ctx);
    walk.types
}

/// The state one [`process_expr`] call carries through the expression.
#[derive(Default)]
struct ExprWalk<'b> {
    types: ExprTypes,
    /// Write-free sub-expressions evaluated since the last write, outermost
    /// only.  If a write follows, they read a scope the caller will not
    /// see, so they are typed against it before the write lands.
    pending: Vec<&'b Expression<'b>>,
    /// Where the innermost fork's entries in `pending` start.  A write
    /// inside a fork only invalidates what the fork itself evaluated: the
    /// entries below were evaluated on the scope the fork was copied from,
    /// which the write does not touch.
    base: usize,
}

impl<'b> ExprWalk<'b> {
    /// A long `->method()` chain recurses through here once per link,
    /// thousands of levels deep, so every byte of this frame is paid at
    /// every level.  The cases that hold values of their own (a copy of the
    /// scope, a resolved type) are `#[inline(never)]` helpers for that
    /// reason.
    fn expr(
        &mut self,
        expr: &'b Expression<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let result = match expr {
            Expression::Assignment(assignment) => self.assignment(expr, assignment, scope, ctx),
            Expression::Parenthesized(inner) => self.expr(inner.expression, scope, ctx),
            Expression::UnaryPrefix(prefix) => match prefix.operator {
                UnaryPrefixOperator::PreIncrement(_) => self.step(
                    expr,
                    prefix.operand,
                    IncrementDecrementKind::Increment,
                    true,
                    scope,
                    ctx,
                ),
                UnaryPrefixOperator::PreDecrement(_) => self.step(
                    expr,
                    prefix.operand,
                    IncrementDecrementKind::Decrement,
                    true,
                    scope,
                    ctx,
                ),
                // `if (!$x = expr())` parses as `!($x = expr())`.
                _ => self.expr(prefix.operand, scope, ctx),
            },
            Expression::UnaryPostfix(postfix) => {
                let step = match postfix.operator {
                    UnaryPostfixOperator::PostIncrement(_) => IncrementDecrementKind::Increment,
                    UnaryPostfixOperator::PostDecrement(_) => IncrementDecrementKind::Decrement,
                };
                self.step(expr, postfix.operand, step, false, scope, ctx)
            }
            Expression::Binary(bin) => match bin.operator {
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => {
                    self.chain(expr, ChainKind::And, scope, ctx)
                }
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => {
                    self.chain(expr, ChainKind::Or, scope, ctx)
                }
                BinaryOperator::NullCoalesce(_) => self.coalesce(bin, scope, ctx),
                _ => {
                    let lhs = self.expr(bin.lhs, scope, ctx);
                    let rhs = self.expr(bin.rhs, scope, ctx);
                    lhs.or(rhs)
                }
            },
            // The receiver of a member access or an offset read is evaluated
            // before the access: `($x = f())->prop`, `($x = $map[$k])[0]`.
            Expression::Access(Access::Property(pa)) => self.expr(pa.object, scope, ctx),
            Expression::Access(Access::NullSafeProperty(pa)) => self.expr(pa.object, scope, ctx),
            Expression::ArrayAccess(aa) => {
                let array = self.expr(aa.array, scope, ctx);
                let index = self.expr(aa.index, scope, ctx);
                array.or(index)
            }
            Expression::Call(call) => self.call(call, scope, ctx),
            Expression::Match(match_expr) => self.match_arms(match_expr, scope, ctx),
            Expression::Conditional(conditional) => self.conditional(conditional, scope, ctx),
            Expression::Instantiation(inst) => match &inst.argument_list {
                Some(args) => self.arguments(args, scope, ctx),
                None => ExprResult::default(),
            },
            Expression::Array(array) => self.elements(array.elements.iter(), scope, ctx),
            Expression::LegacyArray(array) => self.elements(array.elements.iter(), scope, ctx),
            _ => ExprResult::default(),
        };
        self.done(expr, result);
        result
    }

    /// Note that `expr` has been evaluated.  A write-free expression
    /// replaces the pending entries inside it: typing it as a whole types
    /// them too.
    fn done(&mut self, expr: &'b Expression<'b>, result: ExprResult) {
        if result.wrote || matches!(expr, Expression::Literal(_)) {
            return;
        }
        self.drop_pending_inside(expr);
        self.pending.push(expr);
    }

    fn drop_pending_inside(&mut self, expr: &Expression<'_>) {
        let span = expr.span();
        while self.pending.len() > self.base
            && self.pending.last().is_some_and(|last| {
                let inner = last.span();
                inner.start.offset >= span.start.offset && inner.end.offset <= span.end.offset
            })
        {
            self.pending.pop();
        }
    }

    /// `node` is about to change `scope`.  Whatever was evaluated before
    /// it is typed against the scope it actually read.
    fn write(&mut self, node: &Expression<'_>, scope: &ScopeState, ctx: &ForwardWalkCtx<'_>) {
        // What `node` evaluated on its way to the write is typed by the
        // write itself (the value it records), so it needs nothing of its own.
        self.drop_pending_inside(node);
        self.flush(scope, ctx);
    }

    /// Type every pending entry of the current fork against `scope`.
    fn flush(&mut self, scope: &ScopeState, ctx: &ForwardWalkCtx<'_>) {
        let ExprWalk {
            types,
            pending,
            base,
        } = self;
        for expr in pending.drain(*base..) {
            let resolved = resolve_rhs_with_types(expr, scope, ctx, Some(types));
            types.insert(expr, resolved);
        }
    }

    /// Start walking a sub-expression that may not run, on its own copy of
    /// the scope.  Returns what [`Self::leave_fork`] restores.
    fn enter_fork(&mut self) -> usize {
        std::mem::replace(&mut self.base, self.pending.len())
    }

    /// Finish a fork started by [`Self::enter_fork`].  If it wrote, what it
    /// evaluated after its last write read `fork_scope`.
    ///
    /// The fork's owner then joins `fork_scope` into the scope the fork was
    /// copied from, and must [`Self::flush`] that scope first whenever the
    /// join can change it.
    fn leave_fork(
        &mut self,
        saved: usize,
        result: ExprResult,
        fork_scope: &ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) {
        if result.wrote {
            self.flush(fork_scope, ctx);
        }
        self.base = saved;
    }

    /// Walk `expr` on `fork`, a copy of the enclosing scope.
    fn fork(
        &mut self,
        expr: &'b Expression<'b>,
        fork: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let saved = self.enter_fork();
        let result = self.expr(expr, fork, ctx);
        self.leave_fork(saved, result, fork, ctx);
        result
    }

    /// Record the value an assignment yields, and the write it makes.
    #[inline(never)]
    fn assignment(
        &mut self,
        expr: &'b Expression<'b>,
        assignment: &'b Assignment<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        // The assigned value runs first, and can itself assign:
        // `if ($a = $b = expr())`, `if ($a = ($b = f())->m())`.
        self.expr(assignment.rhs, scope, ctx);
        if assignment.operator.is_assign() {
            if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                let value = resolve_rhs_with_types(assignment.rhs, scope, ctx, Some(&self.types));
                self.write(expr, scope, ctx);
                if !value.is_empty() {
                    scope.set(bytes_to_str(dv.name), value.clone());
                }
                self.types.insert(expr, value);
            }
        } else {
            // `??=`, `.=`, `+=`, … write their target here exactly as they
            // do from a statement, so the statement handler decides what
            // the target ends up holding.  Reading `??=` as "no assignment
            // happened" left the target on the type it had going in, which
            // for the `$x ??= …` idiom is the `null` the fallback exists
            // to replace.
            self.write(expr, scope, ctx);
            process_compound_assignment(assignment, scope, ctx);
            if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                self.types
                    .insert(expr, scope.get(bytes_to_str(dv.name)).to_vec());
            }
        }
        record_scope_snapshot(expr.span().end.offset, scope);
        ExprResult { wrote: true }
    }

    /// `++` / `--`: the operand holds the stepped value from here on, and
    /// the expression yields the new value (prefix) or the old one
    /// (postfix).
    #[inline(never)]
    fn step(
        &mut self,
        expr: &'b Expression<'b>,
        operand: &'b Expression<'b>,
        step: IncrementDecrementKind,
        prefix: bool,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let operand_result = self.expr(operand, scope, ctx);
        let Expression::Variable(Variable::Direct(dv)) = operand else {
            return operand_result;
        };
        let name = bytes_to_str(dv.name);
        let old = scope.get(name).to_vec();
        if old.is_empty() {
            return operand_result;
        }
        let current = ResolvedType::types_joined(&old);
        let stepped = type_after_increment_decrement(&current, step);
        self.write(expr, scope, ctx);
        if stepped != current {
            scope.set(name, vec![ResolvedType::from_type_string(stepped)]);
        }
        let value = if prefix {
            scope.get(name).to_vec()
        } else {
            old
        };
        self.types.insert(expr, value);
        record_scope_snapshot(expr.span().end.offset, scope);
        ExprResult { wrote: true }
    }

    /// A `&&` or `||` chain: each operand after the first runs only when
    /// every one before it was truthy (`&&`) or falsy (`||`), so it runs
    /// on the scope the operands before it left, narrowed by what they
    /// proved.  Snapshots record that scope at each operand's start, so a
    /// lookup inside it sees both the narrowing and the writes:
    /// `if (($a = g()) && $a->bar())`.
    #[inline(never)]
    fn chain(
        &mut self,
        expr: &'b Expression<'b>,
        kind: ChainKind,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let operands = match kind {
            ChainKind::And => collect_and_chain_operands(expr),
            ChainKind::Or => collect_or_chain_operands(expr),
        };
        let Some((first, rest)) = operands.split_first() else {
            return ExprResult::default();
        };
        let mut result = self.expr(first, scope, ctx);
        if rest.is_empty() {
            return result;
        }

        let diagnostics = is_diagnostic_scope_active();
        let mut path = scope.clone();
        narrow_by_operand(kind, first, &mut path, ctx);
        for (i, operand) in rest.iter().enumerate() {
            if diagnostics {
                record_scope_snapshot(operand.span().start.offset, &path);
                record_scope_snapshot_recursive(operand, &path);
            }
            let operand_result = self.fork(operand, &mut path, ctx);
            if operand_result.wrote {
                // The chain can stop before this operand, so what it wrote
                // joins the path where it never ran.
                result.wrote = true;
                self.flush(scope, ctx);
                scope.merge_branch(&path);
            }
            if i + 1 < rest.len() {
                narrow_by_operand(kind, operand, &mut path, ctx);
            }
        }
        result
    }

    /// `$a ?? $b`: the right operand runs only when the left one is null.
    #[inline(never)]
    fn coalesce(
        &mut self,
        bin: &'b Binary<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let mut result = self.expr(bin.lhs, scope, ctx);
        let mut ran = scope.clone();
        if self.fork(bin.rhs, &mut ran, ctx).wrote {
            result.wrote = true;
            self.flush(scope, ctx);
            scope.merge_branch(&ran);
        }
        result
    }

    fn call(
        &mut self,
        call: &'b Call<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        // The callee and the receiver run before the arguments do.
        let (receiver, args) = match call {
            Call::Function(fc) => (self.expr(fc.function, scope, ctx), &fc.argument_list),
            Call::Method(mc) => (self.expr(mc.object, scope, ctx), &mc.argument_list),
            Call::NullSafeMethod(mc) => {
                let receiver = self.expr(mc.object, scope, ctx);
                if mc.argument_list.arguments.is_empty() {
                    return receiver;
                }
                return receiver.or(self.nullsafe_arguments(mc, scope, ctx));
            }
            Call::StaticMethod(sc) => (ExprResult::default(), &sc.argument_list),
        };
        receiver.or(self.arguments(args, scope, ctx))
    }

    /// The arguments of a `?->` call run only when the receiver is not
    /// null, so they see it narrowed, and what they write is joined with
    /// the path where the call short-circuited and wrote nothing.
    #[inline(never)]
    fn nullsafe_arguments(
        &mut self,
        call: &'b NullSafeMethodCall<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let diagnostics = is_diagnostic_scope_active();
        let mut ran = scope.clone();
        narrow_nullsafe_call_receiver(call.object, &mut ran, ctx);
        let saved = self.enter_fork();
        let mut result = ExprResult::default();
        for arg in call.argument_list.arguments.iter() {
            let value = arg.value();
            if diagnostics {
                record_scope_snapshot(value.span().start.offset, &ran);
                record_scope_snapshot_recursive(value, &ran);
            }
            result = result.or(self.expr(value, &mut ran, ctx));
        }
        self.leave_fork(saved, result, &ran, ctx);
        if result.wrote {
            self.flush(scope, ctx);
        }
        scope.merge_branch(&ran);
        // What follows the call reads the receiver as it was.
        if diagnostics {
            record_scope_snapshot(call.argument_list.span().end.offset, scope);
        }
        result
    }

    fn arguments(
        &mut self,
        args: &'b ArgumentList<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let mut result = ExprResult::default();
        for arg in args.arguments.iter() {
            result = result.or(self.expr(arg.value(), scope, ctx));
        }
        result
    }

    fn elements(
        &mut self,
        elements: impl Iterator<Item = &'b ArrayElement<'b>>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let mut result = ExprResult::default();
        for element in elements {
            match element {
                ArrayElement::KeyValue(kv) => {
                    result = result.or(self.expr(kv.key, scope, ctx));
                    result = result.or(self.expr(kv.value, scope, ctx));
                }
                ArrayElement::Value(v) => result = result.or(self.expr(v.value, scope, ctx)),
                ArrayElement::Variadic(v) => result = result.or(self.expr(v.value, scope, ctx)),
                ArrayElement::Missing(_) => {}
            }
        }
        result
    }

    /// Only one arm of a `match` runs, so each arm is walked against its
    /// own copy of the scope and the copies are joined: an arm that does
    /// not run cannot leak a definite assignment.  A match with no matching
    /// arm throws `UnhandledMatchError` rather than falling through, so
    /// unlike a `switch` without `default` there is no "no arm ran" scope
    /// to fold into the join.
    #[inline(never)]
    fn match_arms(
        &mut self,
        match_expr: &'b Match<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let subject = self.expr(match_expr.expression, scope, ctx);
        let mut arms = ExprResult::default();
        let mut merged: Option<ScopeState> = None;
        for arm in match_expr.arms.iter() {
            let mut arm_scope = scope.clone();
            arms = arms.or(self.fork(arm.expression(), &mut arm_scope, ctx));
            match &mut merged {
                Some(merged) => merged.merge_branch(&arm_scope),
                None => merged = Some(arm_scope),
            }
        }
        if let Some(merged) = merged {
            if arms.wrote {
                self.flush(scope, ctx);
            }
            *scope = merged;
        }
        subject.or(arms)
    }

    /// Only one branch of a ternary runs; see [`Self::match_arms`].
    #[inline(never)]
    fn conditional(
        &mut self,
        conditional: &'b Conditional<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let condition = self.expr(conditional.condition, scope, ctx);
        let mut then_scope = scope.clone();
        let mut arms = ExprResult::default();
        if let Some(then_expr) = conditional.then {
            arms = self.fork(then_expr, &mut then_scope, ctx);
        }
        let mut else_scope = scope.clone();
        arms = arms.or(self.fork(conditional.r#else, &mut else_scope, ctx));
        if arms.wrote {
            self.flush(scope, ctx);
        }
        then_scope.merge_branch(&else_scope);
        *scope = then_scope;
        condition.or(arms)
    }
}

impl ExprResult {
    fn or(self, other: ExprResult) -> ExprResult {
        ExprResult {
            wrote: self.wrote || other.wrote,
        }
    }
}

/// Narrow `path` by what one chain operand proved for the operands after
/// it: that it was truthy (`&&`) or falsy (`||`).
fn narrow_by_operand<'b>(
    kind: ChainKind,
    operand: &'b Expression<'b>,
    path: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match kind {
        ChainKind::And => apply_condition_narrowing(operand, path, ctx),
        ChainKind::Or => apply_condition_narrowing_inverse(operand, path, ctx),
    }
}
