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
//! - Each node records the scope it runs on at its start (see
//!   [`record_node_scope`]), so a diagnostic lookup anywhere in the
//!   expression finds what was written and proved before that point.  A
//!   walk to the cursor keeps the scope of the innermost node holding the
//!   cursor instead (see [`CursorScope`]), which is what hover and
//!   completion read there.
//! - A call changes the scope where it runs, once its receiver and
//!   arguments have: the variables it writes through by-reference
//!   parameters, and what was known about an object it may have changed.
//! - A condition ([`process_condition`]) also hands back what each of its
//!   outcomes proves, as a [`Condition`].  The right operand of `&&` /
//!   `||` runs on the outcome of the left one that lets it run, and the
//!   arms of a ternary or `match` on the outcome that selects them.  The
//!   right operand of `??` and those arms run on their own copy of the
//!   scope, joined with the path where they did not run when they wrote.
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
        let span = expr.span();
        self.insert_spanned((span.start.offset, span.end.offset), types);
    }

    /// [`Self::insert`] for a node whose span the caller already knows.
    fn insert_spanned(&mut self, (start, end): (u32, u32), types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        self.0.push((start, end, types));
    }
}

/// What processing one expression did.
#[derive(Clone, Copy, Default)]
pub(crate) struct ExprResult {
    /// The expression wrote to the scope somewhere inside it.
    pub(crate) wrote: bool,
}

/// Walk `expr` in evaluation order, applying the writes it makes to
/// `scope` and recording the scopes described in the module docs.
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

/// Walk a condition in evaluation order, applying the writes it makes to
/// `scope`, and return what each of its outcomes proves.
///
/// `scope` is left holding what evaluating the condition left behind,
/// whichever way it came out; the [`Condition`] hands out the scope for
/// each outcome.
pub(crate) fn process_condition<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Condition<'b> {
    ExprWalk::default().cond(condition, scope, ctx, &[])
}

/// The state one [`process_expr`] call carries through the expression.
#[derive(Default)]
struct ExprWalk<'b> {
    types: ExprTypes,
    /// Write-free sub-expressions evaluated since the last write, outermost
    /// only, with their spans.  If a write follows, they read a scope the
    /// caller will not see, so they are typed against it before the write
    /// lands.
    pending: Vec<(&'b Expression<'b>, (u32, u32))>,
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
        record_node_scope(expr.span().start.offset, scope);
        if let Some(cursor_scope) = ctx.cursor_scope {
            cursor_scope.reached(expr, scope, ctx.cursor_offset);
        }
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
                BinaryOperator::And(_)
                | BinaryOperator::LowAnd(_)
                | BinaryOperator::Or(_)
                | BinaryOperator::LowOr(_) => self.short_circuit_value(expr, scope, ctx),
                BinaryOperator::NullCoalesce(_) => self.coalesce(bin, scope, ctx),
                _ => self.operand_chain(bin, scope, ctx),
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
            Expression::Call(call) => {
                let evaluated = self.call(call, scope, ctx);
                evaluated.or(self.effects(expr, scope, ctx))
            }
            Expression::Match(match_expr) => self.match_arms(expr, match_expr, scope, ctx),
            Expression::Conditional(conditional) => self.conditional(expr, conditional, scope, ctx),
            // A `throw` is an expression since PHP 8, and the value it throws
            // is built like any other.
            Expression::Throw(throw_expr) => self.expr(throw_expr.exception, scope, ctx),
            Expression::Instantiation(inst) => match &inst.argument_list {
                Some(args) => {
                    let evaluated = self.arguments(args, scope, ctx);
                    evaluated.or(self.effects(expr, scope, ctx))
                }
                None => ExprResult::default(),
            },
            Expression::Array(array) => self.elements(array.elements.iter(), scope, ctx),
            Expression::LegacyArray(array) => self.elements(array.elements.iter(), scope, ctx),
            Expression::Construct(construct) if let Some(path) = include_path_expr(construct) => {
                self.include(expr, path, scope, ctx)
            }
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
        let span = expr.span();
        self.push_pending(expr, (span.start.offset, span.end.offset));
    }

    /// [`Self::done`] for a node whose span the caller already knows.
    fn done_spanned(&mut self, expr: &'b Expression<'b>, span: (u32, u32), result: ExprResult) {
        if result.wrote || matches!(expr, Expression::Literal(_)) {
            return;
        }
        self.push_pending(expr, span);
    }

    fn push_pending(&mut self, expr: &'b Expression<'b>, span: (u32, u32)) {
        self.drop_pending_within(span);
        self.pending.push((expr, span));
    }

    fn drop_pending_inside(&mut self, expr: &Expression<'_>) {
        let span = expr.span();
        self.drop_pending_within((span.start.offset, span.end.offset));
    }

    fn drop_pending_within(&mut self, (start, end): (u32, u32)) {
        while self.pending.len() > self.base
            && self
                .pending
                .last()
                .is_some_and(|(_, (inner_start, inner_end))| {
                    *inner_start >= start && *inner_end <= end
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
        for (expr, span) in pending.drain(*base..) {
            let resolved = resolve_rhs_with_types(expr, scope, ctx, Some(types));
            types.insert_spanned(span, resolved);
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

    /// Walk `expr`, which may not run at all (the right operand of `??`, an
    /// arm of a ternary or `match`), on `fork`, its own copy of the scope.
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
        if let Some(name) = variable_variable_write_target(assignment.lhs) {
            self.write(expr, scope, ctx);
            widen_variable_variable_write(name, scope);
            record_scope_snapshot(expr.span().end.offset, scope);
            return ExprResult { wrote: true };
        }
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
        // A cursor on the target asks about what was written there.
        if let Some(cursor_scope) = ctx.cursor_scope {
            cursor_scope.reached(assignment.lhs, scope, ctx.cursor_offset);
        }
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

    /// An operator that evaluates both of its operands, in order:
    /// `"a" . $b`, `$n + 1`, `$x === $y`.
    ///
    /// These are left-associative, so `"a" . "b" . "c"` nests on the left,
    /// and generated code can chain thousands of them.  Walk the left spine
    /// in a loop instead of a frame per operator.  Entering each operator
    /// below `bin` in turn, before its first operand, is what recursing
    /// into each did: nothing runs in between.
    #[inline(never)]
    fn operand_chain(
        &mut self,
        bin: &'b Binary<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let mut spine: Vec<(&'b Expression<'b>, &'b Binary<'b>)> = Vec::new();
        let mut leftmost = bin.lhs;
        while let Expression::Binary(inner) = leftmost
            && evaluates_both_operands(&inner.operator)
        {
            spine.push((leftmost, inner));
            leftmost = inner.lhs;
        }
        // Every operator in the spine starts where its first operand does.
        let start = leftmost.span().start.offset;
        for (node, _) in &spine {
            record_node_scope(start, scope);
            if let Some(cursor_scope) = ctx.cursor_scope {
                let (start, end) = span_from(node, start);
                cursor_scope.reached_span(start, end, scope, ctx.cursor_offset);
            }
        }
        let mut result = self.expr(leftmost, scope, ctx);
        for (node, inner) in spine.into_iter().rev() {
            result = result.or(self.expr(inner.rhs, scope, ctx));
            self.done_spanned(node, span_from(node, start), result);
        }
        result.or(self.expr(bin.rhs, scope, ctx))
    }

    /// A `&&` / `||` whose value is used rather than tested: its operands
    /// still short-circuit, so it is walked as the condition it is.
    ///
    /// Kept out of line so the [`Condition`] it builds is not part of every
    /// [`Self::expr`] frame.
    #[inline(never)]
    fn short_circuit_value(
        &mut self,
        expr: &'b Expression<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        ExprResult {
            wrote: self.cond(expr, scope, ctx, &[]).wrote,
        }
    }

    /// Walk a condition, see [`process_condition`].
    ///
    /// `pinned` names the subjects the conjuncts before this condition
    /// pinned to a definite class (see [`Condition::or`]).
    fn cond(
        &mut self,
        expr: &'b Expression<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
        pinned: &[String],
    ) -> Condition<'b> {
        // `&&` and `||` are left-associative, so `a && b && c` nests on the
        // left, and generated code can chain thousands of them.  Walk the
        // left spine down to its first operand in a loop, then build the
        // operators back up, instead of a frame per operator.  The left
        // operand of each starts on the scope the whole condition started
        // on, so entering them in turn is what recursing into each did.
        type SpineEntry<'b> = (
            &'b Expression<'b>,
            (u32, u32),
            &'b Expression<'b>,
            Option<ScopeState>,
        );
        let mut spine: Vec<SpineEntry<'b>> = Vec::new();
        let mut node = expr;
        let mut span = span_from(node, expr.span().start.offset);
        let mut seeded = false;
        let first = loop {
            record_node_scope(span.0, scope);
            if let Some(cursor_scope) = ctx.cursor_scope {
                cursor_scope.reached_span(span.0, span.1, scope, ctx.cursor_offset);
            }
            match split_condition(node) {
                Split::Leaf(leaf) => break self.leaf(leaf, scope, ctx, pinned),
                Split::Not(inner) => break Condition::not(self.cond(inner, scope, ctx, &[])),
                Split::And(lhs, rhs) => {
                    spine.push((node, span, rhs, None));
                    span = left_operand_span(node, lhs, span.0);
                    node = lhs;
                }
                Split::Or(lhs, rhs) => {
                    // What a leg concludes about a member path is checked
                    // against what the scope already knew about it, so the
                    // paths every leg names are seeded before the legs split.
                    // The outermost `||` of the spine names every path an
                    // inner one does, and seeding skips a path already in
                    // scope, so seeding once is enough.
                    if !seeded {
                        seed_property_keys_into_scope(node, scope, ctx);
                        seeded = true;
                    }
                    spine.push((node, span, rhs, Some(scope.clone())));
                    span = left_operand_span(node, lhs, span.0);
                    node = lhs;
                }
            }
        };
        self.done_spanned(node, span, ExprResult { wrote: first.wrote });
        let mut condition = first;
        for (node, span, rhs, base) in spine.into_iter().rev() {
            let and = base.is_none();
            let right = self.short_circuit(and, &condition, rhs, scope, ctx, pinned);
            condition = match base {
                None => Condition::and(condition, right),
                Some(base) => Condition::or(condition, right, base, pinned.to_vec()),
            };
            self.done_spanned(
                node,
                span,
                ExprResult {
                    wrote: condition.wrote,
                },
            );
        }
        condition
    }

    /// One operand of a condition that is not itself `&&`, `||` or `!`.
    fn leaf(
        &mut self,
        leaf: &'b Expression<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
        pinned: &[String],
    ) -> Condition<'b> {
        let wrote = self.expr(leaf, scope, ctx).wrote;
        Condition::leaf(leaf, scope.clone(), wrote, pinned)
    }

    /// The right operand of `lhs && rhs` (`and`) or `lhs || rhs`, given
    /// what `lhs` proved: it runs only when the left one was truthy (`&&`)
    /// or falsy (`||`), so it runs on the scope the left one left in that
    /// outcome, and a lookup inside it sees both the narrowing and the
    /// writes: `if (($a = g()) && $a->bar())`.
    #[inline(never)]
    fn short_circuit(
        &mut self,
        and: bool,
        left: &Condition<'b>,
        rhs: &'b Expression<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
        pinned: &[String],
    ) -> Condition<'b> {
        let (mut path, rhs_pinned) = if and {
            let rhs_pinned = with_pinned(pinned, left.pinned(ctx));
            (left.truthy(ctx).clone(), rhs_pinned)
        } else {
            (left.falsey(ctx).clone(), pinned.to_vec())
        };
        let saved = self.enter_fork();
        let right = self.cond(rhs, &mut path, ctx, &rhs_pinned);
        self.leave_fork(saved, ExprResult { wrote: right.wrote }, &path, ctx);
        if right.wrote {
            // The condition can stop before the right operand, so what it
            // wrote joins the path where it never ran.
            self.flush(scope, ctx);
            scope.merge_branch(&path);
        }
        right
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
        // A plain variable that was `null` is falsy, which may prove
        // something about the variables tested alongside it.  An offset or
        // a property may be missing instead, which its falsy narrowing
        // does not describe.
        // A variable that may be unset is `null` whatever its type says, so
        // a narrowing that finds no falsy value to keep proves nothing.
        if matches!(bin.lhs, Expression::Variable(Variable::Direct(_))) {
            let mut narrowed = ran.clone();
            narrow_leaf_falsey(bin.lhs, &mut narrowed, ctx);
            if !narrowed.unreachable {
                ran = narrowed;
            }
        }
        if self.fork(bin.rhs, &mut ran, ctx).wrote {
            result.wrote = true;
            self.flush(scope, ctx);
            scope.merge_branch(&ran);
        }
        // The right operand's scope was recorded inside it; what follows
        // the `??` reads the scope it left behind, not that fork.
        record_node_scope(bin.span().end.offset, scope);
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

    /// What the call (or instantiation) `expr` changes besides producing
    /// its value, once its receiver and arguments have run: the variables
    /// it writes through by-reference parameters (see
    /// [`process_pass_by_ref`]), what was known about an object it changed
    /// (see [`process_call_effects`]), and the type a `@phpstan-self-out`
    /// method leaves its receiver with (see [`process_self_out_narrowing`])
    /// or a Doctrine query builder call leaves the builder's variable with
    /// (see [`process_dql_builder_call`]).
    #[inline(never)]
    fn effects(
        &mut self,
        expr: &'b Expression<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let mut after = scope.clone();
        process_pass_by_ref(expr, &mut after, ctx);
        process_call_effects(expr, &mut after, ctx);
        process_self_out_narrowing(expr, &mut after, ctx);
        process_dql_builder_call(expr, &mut after, ctx);
        // A cursor on a variable the call wrote through asks about what the
        // call left there: `$m` in `preg_match('/(a)/', $s, $m)`.
        if let Some(cursor_scope) = ctx.cursor_scope
            && let Some(written) = argument_at_cursor(expr, ctx.cursor_offset)
            && !same_types(scope.get(written), after.get(written))
        {
            cursor_scope.record(&after);
        }
        let wrote = after.locals.differs_from(&scope.locals);
        if wrote {
            // The call's own value, and everything it read on the way, came
            // from the scope before it changed anything.
            self.done(expr, ExprResult::default());
            self.flush(scope, ctx);
        }
        *scope = after;
        ExprResult { wrote }
    }

    /// An `include`/`require`: the path is evaluated, then the included
    /// file's top level runs in this scope and leaves its variables here.
    #[inline(never)]
    fn include(
        &mut self,
        expr: &'b Expression<'b>,
        path: &'b Expression<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let evaluated = self.expr(path, scope, ctx);
        let Some(locals) = include_locals(path, scope, ctx).filter(|locals| !locals.is_empty())
        else {
            return evaluated;
        };
        // The include's own value, and the path it read, came from the
        // scope before the included file changed anything.
        self.done(expr, ExprResult::default());
        self.flush(scope, ctx);
        apply_include_locals(&locals, scope);
        record_scope_snapshot(expr.span().end.offset, scope);
        ExprResult { wrote: true }
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
        let mut ran = scope.clone();
        narrow_nullsafe_call_receiver(call.object, &mut ran, ctx);
        let saved = self.enter_fork();
        let mut result = ExprResult::default();
        for arg in call.argument_list.arguments.iter() {
            result = result.or(self.expr(arg.value(), &mut ran, ctx));
        }
        self.leave_fork(saved, result, &ran, ctx);
        if result.wrote {
            self.flush(scope, ctx);
        }
        scope.merge_branch(&ran);
        // What follows the call reads the receiver as it was.
        record_node_scope(call.argument_list.span().end.offset, scope);
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

    /// Join the scopes the arms of a ternary or `match` left into `scope`,
    /// when one of them wrote.  Arms that did not write leave `scope` as the
    /// expression found it: joining what each arm proved would only widen
    /// the narrowing back to the type it started from.
    ///
    /// Either way, what follows the expression reads the joined scope rather
    /// than the last arm's narrowing.
    fn join_arms(
        &mut self,
        expr: &'b Expression<'b>,
        wrote: bool,
        arms: Vec<ScopeState>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) {
        if wrote {
            self.flush(scope, ctx);
            let mut arms = arms.into_iter();
            if let Some(mut joined) = arms.next() {
                for arm in arms {
                    joined.merge_branch(&arm);
                }
                *scope = joined;
            }
        }
        record_node_scope(expr.span().end.offset, scope);
    }

    /// Only one arm of a `match` runs, so each arm is walked against its
    /// own scope and the scopes are joined: an arm that does not run cannot
    /// leak a definite assignment.  A match with no matching arm throws
    /// `UnhandledMatchError` rather than falling through, so unlike a
    /// `switch` without `default` there is no "no arm ran" scope to fold
    /// into the join.
    ///
    /// An arm runs where the subject was identical to one of its
    /// conditions and to none of the conditions above it, so it sees what
    /// `subject === condition` proves in an `if`, and `default` runs once
    /// every condition failed, wherever it is written.  A `match (true)`
    /// arm runs where one of its conditions held instead.
    #[inline(never)]
    fn match_arms(
        &mut self,
        expr: &'b Expression<'b>,
        match_expr: &'b Match<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let subject = self.expr(match_expr.expression, scope, ctx);
        if match_expr.expression.is_true() {
            return subject.or(self.match_true_arms(expr, match_expr, scope, ctx));
        }
        let mut failed = scope.clone();
        let mut default_arm = None;
        let mut wrote = false;
        let mut arms = Vec::with_capacity(match_expr.arms.len());
        for arm in match_expr.arms.iter() {
            match arm {
                MatchArm::Expression(expr_arm) => {
                    let conditions: Vec<&'b Expression<'b>> =
                        expr_arm.conditions.iter().copied().collect();
                    let mut arm_scope = failed.clone();
                    apply_subject_match_narrowing(
                        match_expr.expression,
                        &conditions,
                        true,
                        &mut arm_scope,
                        ctx,
                    );
                    apply_subject_mismatch_narrowing(
                        match_expr.expression,
                        &conditions,
                        true,
                        &mut failed,
                        ctx,
                    );
                    wrote |= self.fork(expr_arm.expression, &mut arm_scope, ctx).wrote;
                    arms.push(arm_scope);
                }
                MatchArm::Default(def_arm) => default_arm = Some(def_arm.expression),
            }
        }
        if let Some(default_arm) = default_arm {
            wrote |= self.fork(default_arm, &mut failed, ctx).wrote;
            arms.push(failed);
        }
        self.join_arms(expr, wrote, arms, scope, ctx);
        ExprResult {
            wrote: subject.wrote || wrote,
        }
    }

    /// The arms of a `match (true)`.
    ///
    /// Reaching an arm means every condition above it was tested and was
    /// not `true`, so the inverse of each holds in its body, exactly as in
    /// an `elseif` chain; `default` runs only once every condition failed,
    /// wherever it is written.  A condition is itself tested only once
    /// every condition before it failed, so a chain or a ternary inside it
    /// starts from that.
    #[inline(never)]
    fn match_true_arms(
        &mut self,
        expr: &'b Expression<'b>,
        match_expr: &'b Match<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let mut failed = scope.clone();
        let mut default_arm = None;
        let mut wrote = false;
        let mut arms = Vec::with_capacity(match_expr.arms.len());
        for arm in match_expr.arms.iter() {
            match arm {
                MatchArm::Expression(expr_arm) => {
                    let mut arm_scope = failed.clone();
                    apply_match_arm_narrowing(expr_arm, &mut arm_scope, ctx);
                    for condition in expr_arm.conditions.iter() {
                        self.fork(condition, &mut failed.clone(), ctx);
                        apply_failed_match_condition_narrowing(condition, &mut failed, ctx);
                    }
                    wrote |= self.fork(expr_arm.expression, &mut arm_scope, ctx).wrote;
                    arms.push(arm_scope);
                }
                MatchArm::Default(def_arm) => default_arm = Some(def_arm.expression),
            }
        }
        if let Some(default_arm) = default_arm {
            wrote |= self.fork(default_arm, &mut failed, ctx).wrote;
            arms.push(failed);
        }
        self.join_arms(expr, wrote, arms, scope, ctx);
        ExprResult { wrote }
    }

    /// Only one arm of a ternary runs; see [`Self::match_arms`].  Each runs
    /// where the condition came out its way: `$x instanceof Foo ? $x->foo()
    /// : null` reads a `Foo`, and so does the short `$x ?: …`'s value.
    #[inline(never)]
    fn conditional(
        &mut self,
        expr: &'b Expression<'b>,
        conditional: &'b Conditional<'b>,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> ExprResult {
        let condition = self.cond(conditional.condition, scope, ctx, &[]);
        let mut then_scope = condition.truthy(ctx).clone();
        let mut wrote = false;
        if let Some(then_expr) = conditional.then {
            wrote |= self.fork(then_expr, &mut then_scope, ctx).wrote;
        }
        let mut else_scope = condition.falsey(ctx).clone();
        wrote |= self.fork(conditional.r#else, &mut else_scope, ctx).wrote;
        self.join_arms(expr, wrote, vec![then_scope, else_scope], scope, ctx);
        ExprResult {
            wrote: condition.wrote || wrote,
        }
    }
}

impl ExprResult {
    fn or(self, other: ExprResult) -> ExprResult {
        ExprResult {
            wrote: self.wrote || other.wrote,
        }
    }
}

/// Whether the operator always evaluates both of its operands, unlike the
/// short-circuiting `&&`, `||` and `??`.
fn evaluates_both_operands(operator: &BinaryOperator<'_>) -> bool {
    !matches!(
        operator,
        BinaryOperator::And(_)
            | BinaryOperator::LowAnd(_)
            | BinaryOperator::Or(_)
            | BinaryOperator::LowOr(_)
            | BinaryOperator::NullCoalesce(_)
    )
}

/// The span of `expr`, given where it starts.
///
/// `Binary::span` finds an operator's start by walking down its left edge,
/// so asking every operator of a long left-nested chain for its span is
/// quadratic.  Every operator in the chain's spine starts where the chain
/// does, so a walk down the spine knows the start and passes it in.
fn span_from(expr: &Expression<'_>, start: u32) -> (u32, u32) {
    let end = match expr {
        Expression::Binary(bin) => bin.rhs.span().end.offset,
        _ => expr.span().end.offset,
    };
    (start, end)
}

/// The span of `lhs`, the left operand of the condition `node` that
/// starts at `start`.  When `node` is the operator itself, rather than the
/// operator wrapped in parentheses or a cast, the two start together.
fn left_operand_span(node: &Expression<'_>, lhs: &Expression<'_>, start: u32) -> (u32, u32) {
    let start = match node {
        Expression::Binary(bin) if std::ptr::eq(bin.lhs, lhs) => start,
        _ => lhs.span().start.offset,
    };
    span_from(lhs, start)
}

/// The variable passed directly as an argument of the call (or
/// instantiation) `expr` that holds the cursor, if any.
fn argument_at_cursor<'b>(expr: &'b Expression<'b>, cursor: u32) -> Option<&'b str> {
    let args = match expr {
        Expression::Call(Call::Function(call)) => &call.argument_list,
        Expression::Call(Call::Method(call)) => &call.argument_list,
        Expression::Call(Call::NullSafeMethod(call)) => &call.argument_list,
        Expression::Call(Call::StaticMethod(call)) => &call.argument_list,
        Expression::Instantiation(inst) => inst.argument_list.as_ref()?,
        _ => return None,
    };
    args.arguments.iter().find_map(|arg| match arg.value() {
        Expression::Variable(Variable::Direct(dv)) => {
            let span = dv.span();
            (span.start.offset <= cursor && cursor <= span.end.offset)
                .then(|| bytes_to_str(dv.name))
        }
        _ => None,
    })
}
