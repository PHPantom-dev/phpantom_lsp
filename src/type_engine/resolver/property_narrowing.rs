/// Property-path narrowing: applies instanceof / assert / guard-clause
/// narrowing to a `$this->prop` (or `$obj->prop`) resolution result by
/// walking the enclosing method body from its start down to the cursor.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::types::ClassInfo;

use super::{Loaders, ResolutionCtx, VarResolutionCtx};

thread_local! {
    /// The narrowing walks currently in progress on this thread, each
    /// entry the identity of the source that walk covers.
    ///
    /// Guard-clause narrowing resolves method-call receivers to decide
    /// whether a branch unconditionally exits.  Resolving a chained
    /// call like `$h->getWork()` enters the call-key narrowing path
    /// (`narrowed_call` in `variable/rhs_resolution/mod.rs`), which
    /// calls back into [`apply_property_narrowing`] for the call key.
    /// That nested walk covers the same body, re-encounters the other
    /// guards in it, resolves *their* receivers, and re-enters again
    /// under a different key each time, so a body holding n chained
    /// calls fans out factorially in n.  Six of them measured 1.5 s and
    /// eight never finished.
    ///
    /// A walk is therefore only started when no other walk is running
    /// over the same source: the one already in progress covers the
    /// whole body, and a nested one exists only to sharpen a receiver
    /// type that is being read to answer "does this branch exit".
    /// Blocking it leaves the declared type standing for that one
    /// receiver, which costs a `never`-returning call inside a guard
    /// body going unrecognised when recognising it depended on
    /// narrowing a receiver from another guard in the same body.
    ///
    /// The source is what the entry holds because the same body is not
    /// the only thing a walk can descend into: resolving a receiver
    /// routinely crosses into another file, and a walk over that file
    /// is new work rather than a repeat of the one that asked for it.
    ///
    /// Entries are pushed and popped in call order, so this is a stack
    /// rather than a set: at realistic nesting depths a linear scan
    /// beats hashing, and it keeps the walk down to one allocation.
    static NARROWING_IN_PROGRESS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// RAII guard that pops the entry [`apply_property_narrowing`] pushed
/// onto [`NARROWING_IN_PROGRESS`], including on panic.  A leaked entry
/// would silently disable narrowing for that subject for the rest of
/// the thread's life.
struct NarrowingGuard;

impl Drop for NarrowingGuard {
    fn drop(&mut self) {
        NARROWING_IN_PROGRESS.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// Apply instanceof / assert narrowing for a property-access path.
///
/// This is the property-level analog of the narrowing that
/// [`crate::type_engine::variable::resolution::resolve_variable_in_statements`]
/// performs for plain variables.  It re-parses the source, locates
/// the enclosing method body, and walks its statements with a
/// [`VarResolutionCtx`] whose `var_name` is the full property path
/// (e.g. `$this->timeline`).  The existing narrowing functions in
/// [`crate::type_engine::types::narrowing`] already support property paths
/// via [`crate::type_engine::types::narrowing::expr_to_subject_key`], so no
/// changes to those functions are required.
pub(crate) fn apply_property_narrowing(
    property_path: &str,
    current_class: &ClassInfo,
    rctx: &ResolutionCtx<'_>,
    results: &mut Vec<Arc<ClassInfo>>, // still operates on Arc<ClassInfo> — called from property chain
) -> bool {
    use crate::parser::with_parsed_program;

    // Break the re-entry cycle described on `NARROWING_IN_PROGRESS`,
    // leaving `results` as the caller passed them so the declared type
    // stands.
    let source = rctx.content.as_ptr() as usize;
    let entered = NARROWING_IN_PROGRESS.with(|stack| {
        let mut stack = stack.borrow_mut();
        if stack.contains(&source) {
            return false;
        }
        stack.push(source);
        true
    });
    if !entered {
        return false;
    }
    let _guard = NarrowingGuard;

    // The narrowing walk functions operate on Vec<ClassInfo>, so unwrap
    // the Arcs, run narrowing, then re-wrap.
    let mut plain: Vec<ClassInfo> = results.drain(..).map(Arc::unwrap_or_clone).collect();
    let mut is_intersection = false;

    with_parsed_program(
        rctx.content,
        "apply_property_narrowing",
        |program, _content| {
            let ctx = VarResolutionCtx {
                var_name: property_path,
                current_class,
                all_classes: rctx.all_classes,
                content: rctx.content,
                cursor_offset: rctx.cursor_offset,
                class_loader: rctx.class_loader,
                backend: rctx.backend,
                loaders: Loaders::with_function(rctx.function_loader),
                resolved_class_cache: crate::virtual_members::active_resolved_class_cache(),
                enclosing_return_type: None,
                top_level_scope: None,
                branch_aware: false,
                match_arm_narrowing: HashMap::new(),
                scope_var_resolver: None,
                scope_proofs: None,
            };
            walk_property_narrowing_in_statements(
                program.statements.iter(),
                &ctx,
                &mut plain,
                &mut is_intersection,
            );
        },
    );

    *results = plain.into_iter().map(Arc::new).collect();
    is_intersection
}

/// Walk top-level statements to find the class + method containing the
/// cursor, then apply narrowing to `results` for the given property path.
fn walk_property_narrowing_in_statements<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::cst::Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    is_intersection: &mut bool,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    for stmt in statements {
        match stmt {
            Statement::Class(class) => {
                let start = class.left_brace.start.offset;
                let end = class.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    walk_property_narrowing_in_members(
                        class.members.iter(),
                        ctx,
                        results,
                        is_intersection,
                    );
                    return;
                }
            }
            Statement::Trait(trait_def) => {
                let start = trait_def.left_brace.start.offset;
                let end = trait_def.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    walk_property_narrowing_in_members(
                        trait_def.members.iter(),
                        ctx,
                        results,
                        is_intersection,
                    );
                    return;
                }
            }
            Statement::Namespace(ns) => {
                let ns_span = ns.span();
                if ctx.cursor_offset >= ns_span.start.offset
                    && ctx.cursor_offset <= ns_span.end.offset
                {
                    walk_property_narrowing_in_statements(
                        ns.statements().iter(),
                        ctx,
                        results,
                        is_intersection,
                    );
                    return;
                }
            }
            Statement::Function(func) => {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                if ctx.cursor_offset >= body_start && ctx.cursor_offset <= body_end {
                    let floor = stale_narrowing_floor(func.body.statements.iter(), ctx);
                    walk_property_narrowing_stmts(
                        func.body.statements.iter(),
                        ctx,
                        results,
                        floor,
                        is_intersection,
                    );
                    return;
                }
            }
            // ── Functions inside if-guards / blocks ──
            // The common PHP pattern `if (! function_exists('foo'))
            // { function foo(…) { … } }` nests the function
            // declaration inside an if body.  Recurse into blocks
            // and if-bodies so property narrowing still works.
            Statement::If(if_stmt) => {
                let if_span = stmt.span();
                if ctx.cursor_offset >= if_span.start.offset
                    && ctx.cursor_offset <= if_span.end.offset
                {
                    for inner in if_stmt.body.statements().iter() {
                        walk_property_narrowing_in_statements(
                            std::iter::once(inner),
                            ctx,
                            results,
                            is_intersection,
                        );
                    }
                }
            }
            Statement::Block(block) => {
                let blk_span = stmt.span();
                if ctx.cursor_offset >= blk_span.start.offset
                    && ctx.cursor_offset <= blk_span.end.offset
                {
                    walk_property_narrowing_in_statements(
                        block.statements.iter(),
                        ctx,
                        results,
                        is_intersection,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Walk class members to find the method containing the cursor, then
/// apply instanceof / guard-clause narrowing for the property path.
fn walk_property_narrowing_in_members<'b>(
    members: impl Iterator<Item = &'b mago_syntax::cst::class_like::member::ClassLikeMember<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    is_intersection: &mut bool,
) {
    if let Some(block) =
        crate::util::find_enclosing_method_block_in_members(members, ctx.cursor_offset)
    {
        let floor = stale_narrowing_floor(block.statements.iter(), ctx);
        walk_property_narrowing_stmts(
            block.statements.iter(),
            ctx,
            results,
            floor,
            is_intersection,
        );
    }
}

/// Walk statements applying only narrowing (no assignment scanning)
/// for a property path like `$this->prop`.
///
/// `floor` is the offset returned by [`stale_narrowing_floor`]: every
/// check that ends before it describes a value the subject no longer
/// holds, so it is walked past without being applied.
fn walk_property_narrowing_stmts<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::cst::Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    floor: Option<u32>,
    is_intersection: &mut bool,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    use crate::type_engine::types::narrowing;

    for stmt in statements {
        let stmt_span = stmt.span();
        // Only consider statements whose start is before the cursor.
        if stmt_span.start.offset >= ctx.cursor_offset {
            continue;
        }

        match stmt {
            Statement::If(if_stmt) => {
                walk_property_narrowing_if(if_stmt, stmt, ctx, results, floor, is_intersection);
            }
            Statement::Block(block) => {
                walk_property_narrowing_stmts(
                    block.statements.iter(),
                    ctx,
                    results,
                    floor,
                    is_intersection,
                );
            }
            Statement::Expression(expr_stmt) => {
                // assert($this->prop instanceof Foo) — unconditional
                if !check_is_stale(expr_stmt.expression, floor) {
                    let mut shape = narrowing::NarrowedShape::NotApplied;
                    narrowing::try_apply_assert_instanceof_narrowing(
                        expr_stmt.expression,
                        ctx,
                        results,
                        &mut shape,
                    );
                    shape.record(is_intersection);
                }
                // `$x = $this->prop instanceof Foo ? … : …` and other
                // ternaries nested in the expression narrow the property
                // path inside the branch containing the cursor.
                walk_property_narrowing_expr(
                    expr_stmt.expression,
                    ctx,
                    results,
                    floor,
                    is_intersection,
                );
            }
            Statement::Return(ret) => {
                // `return $this->prop instanceof Foo ? … : …` — narrow the
                // property path inside the ternary branch at the cursor.
                if let Some(value) = ret.value {
                    walk_property_narrowing_expr(value, ctx, results, floor, is_intersection);
                }
            }
            // Every loop body reads the same either way it is spelled, so
            // `statements()` covers both the single-statement and the
            // `:`-delimited form.
            Statement::Foreach(loop_stmt) => {
                walk_property_narrowing_stmts(
                    loop_stmt.body.statements().iter(),
                    ctx,
                    results,
                    floor,
                    is_intersection,
                );
            }
            Statement::While(loop_stmt) => {
                walk_property_narrowing_stmts(
                    loop_stmt.body.statements().iter(),
                    ctx,
                    results,
                    floor,
                    is_intersection,
                );
            }
            Statement::For(loop_stmt) => {
                walk_property_narrowing_stmts(
                    loop_stmt.body.statements().iter(),
                    ctx,
                    results,
                    floor,
                    is_intersection,
                );
            }
            Statement::DoWhile(dw) => {
                walk_property_narrowing_stmt(dw.statement, ctx, results, floor, is_intersection);
            }
            Statement::Try(try_stmt) => {
                walk_property_narrowing_stmts(
                    try_stmt.block.statements.iter(),
                    ctx,
                    results,
                    floor,
                    is_intersection,
                );
                for catch in try_stmt.catch_clauses.iter() {
                    walk_property_narrowing_stmts(
                        catch.block.statements.iter(),
                        ctx,
                        results,
                        floor,
                        is_intersection,
                    );
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    walk_property_narrowing_stmts(
                        finally.block.statements.iter(),
                        ctx,
                        results,
                        floor,
                        is_intersection,
                    );
                }
            }
            Statement::Switch(switch) => {
                for case in switch.body.cases().iter() {
                    walk_property_narrowing_stmts(
                        case.statements().iter(),
                        ctx,
                        results,
                        floor,
                        is_intersection,
                    );
                }
            }
            _ => {}
        }
    }
}

/// One branch of an `if` chain, as the property-narrowing walk reads it.
struct NarrowingBranch<'a, 'ast> {
    /// The condition that has to hold for this branch to run, or `None`
    /// for the `else`, which runs when none of them do.
    condition: Option<&'ast mago_syntax::cst::Expression<'ast>>,
    /// The source range the branch covers, which is where a narrowing it
    /// establishes applies.
    span: mago_span::Span,
    /// The statements the branch runs.
    statements: &'a [mago_syntax::cst::Statement<'ast>],
}

/// Apply property-level narrowing inside an if / elseif / else chain.
fn walk_property_narrowing_if<'b>(
    if_stmt: &'b mago_syntax::cst::If<'b>,
    enclosing_stmt: &'b mago_syntax::cst::Statement<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    floor: Option<u32>,
    is_intersection: &mut bool,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    use crate::type_engine::types::narrowing;

    // The two spellings of an `if` differ only in how each branch's
    // statements and source range are found; once those are in hand the
    // narrowing is the same, so both describe their branches the same way
    // and hand them to `walk_narrowing_branches`.
    let branches: Vec<NarrowingBranch<'_, 'b>> = match &if_stmt.body {
        IfBody::Statement(body) => {
            let mut branches = vec![NarrowingBranch {
                condition: Some(if_stmt.condition),
                span: body.statement.span(),
                statements: std::slice::from_ref(body.statement),
            }];
            branches.extend(body.else_if_clauses.iter().map(|else_if| NarrowingBranch {
                condition: Some(else_if.condition),
                span: else_if.statement.span(),
                statements: std::slice::from_ref(else_if.statement),
            }));
            if let Some(else_clause) = &body.else_clause {
                branches.push(NarrowingBranch {
                    condition: None,
                    span: else_clause.statement.span(),
                    statements: std::slice::from_ref(else_clause.statement),
                });
            }
            branches
        }
        IfBody::ColonDelimited(body) => {
            // A `:`-delimited branch has no span of its own, so each one
            // runs from its `:` to wherever the next keyword starts.
            let then_end = if let Some(first) = body.else_if_clauses.first() {
                first.elseif.span().start.offset
            } else if let Some(ref ec) = body.else_clause {
                ec.r#else.span().start.offset
            } else {
                body.endif.span().start.offset
            };
            let mut branches = vec![NarrowingBranch {
                condition: Some(if_stmt.condition),
                span: mago_span::Span::new(
                    body.colon.file_id,
                    body.colon.start,
                    mago_span::Position::new(then_end),
                ),
                statements: body.statements.as_slice(),
            }];
            branches.extend(body.else_if_clauses.iter().map(|else_if| {
                NarrowingBranch {
                    condition: Some(else_if.condition),
                    span: mago_span::Span::new(
                        else_if.colon.file_id,
                        else_if.colon.start,
                        mago_span::Position::new(
                            else_if
                                .statements
                                .span(else_if.colon.file_id, else_if.colon.end)
                                .end
                                .offset,
                        ),
                    ),
                    statements: else_if.statements.as_slice(),
                }
            }));
            if let Some(else_clause) = &body.else_clause {
                branches.push(NarrowingBranch {
                    condition: None,
                    span: mago_span::Span::new(
                        else_clause.colon.file_id,
                        else_clause.colon.start,
                        mago_span::Position::new(
                            else_clause
                                .statements
                                .span(else_clause.colon.file_id, else_clause.colon.end)
                                .end
                                .offset,
                        ),
                    ),
                    statements: else_clause.statements.as_slice(),
                });
            }
            branches
        }
    };

    walk_narrowing_branches(&branches, ctx, results, floor, is_intersection);

    let condition_is_stale = check_is_stale(if_stmt.condition, floor);

    // ── Guard clause narrowing ──
    // When the then-body unconditionally exits and there are no
    // elseif / else branches, apply inverse narrowing after the if.
    if enclosing_stmt.span().end.offset < ctx.cursor_offset && !condition_is_stale {
        narrowing::apply_guard_clause_narrowing(if_stmt, ctx, results);
    }
}

/// Narrow inside each branch of an `if` chain, then walk it.
///
/// A branch's own condition holds inside it; the `else` runs only when
/// every condition above it was false, so each of those is applied
/// inverted there. A condition written before the narrowing floor is
/// stale — it describes a value that has since been reassigned — and
/// proves nothing about the branch.
fn walk_narrowing_branches(
    branches: &[NarrowingBranch<'_, '_>],
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    floor: Option<u32>,
    is_intersection: &mut bool,
) {
    use crate::type_engine::types::narrowing;

    for (index, branch) in branches.iter().enumerate() {
        match branch.condition {
            Some(condition) => {
                if !check_is_stale(condition, floor) {
                    narrowing::try_apply_instanceof_narrowing(condition, branch.span, ctx, results)
                        .record(is_intersection);
                }
            }
            None => {
                for prior in branches[..index].iter().filter_map(|b| b.condition) {
                    if !check_is_stale(prior, floor) {
                        narrowing::try_apply_instanceof_narrowing_inverse(
                            prior,
                            branch.span,
                            ctx,
                            results,
                        )
                        .record(is_intersection);
                    }
                }
            }
        }
        walk_property_narrowing_stmts(
            branch.statements.iter(),
            ctx,
            results,
            floor,
            is_intersection,
        );
    }
}

/// Dispatch a single statement to `walk_property_narrowing_stmts`.
fn walk_property_narrowing_stmt<'b>(
    stmt: &'b mago_syntax::cst::Statement<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    floor: Option<u32>,
    is_intersection: &mut bool,
) {
    walk_property_narrowing_stmts(std::iter::once(stmt), ctx, results, floor, is_intersection);
}

/// Apply property-level narrowing inside ternary (conditional) expressions.
///
/// When the cursor falls inside the then-branch of
/// `$this->prop instanceof Foo ? <then> : <else>`, the property path is
/// narrowed to `Foo`; inside the else-branch the inverse applies. This
/// mirrors the if-statement narrowing in [`walk_property_narrowing_if`]
/// but for ternaries, which can appear anywhere an expression is expected
/// (return values, assignment RHS, call arguments, …). The walk recurses
/// through those containers so a ternary nested inside them is still
/// reached.
fn walk_property_narrowing_expr<'b>(
    expr: &'b mago_syntax::cst::Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    floor: Option<u32>,
    is_intersection: &mut bool,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    use crate::type_engine::types::narrowing;

    // Only descend into the sub-expression that contains the cursor.
    let span = expr.span();
    if ctx.cursor_offset < span.start.offset || ctx.cursor_offset > span.end.offset {
        return;
    }

    match expr {
        Expression::Conditional(cond) => {
            // Full ternary `cond ? then : else`. Narrow the property path
            // in whichever branch holds the cursor. The short form
            // `$x ?: $y` has no `then` branch, so nothing to narrow there.
            let condition_is_stale = check_is_stale(cond.condition, floor);
            if let Some(then_expr) = cond.then {
                let then_span = then_expr.span();
                if ctx.cursor_offset >= then_span.start.offset
                    && ctx.cursor_offset <= then_span.end.offset
                {
                    if !condition_is_stale {
                        narrowing::try_apply_instanceof_narrowing(
                            cond.condition,
                            then_span,
                            ctx,
                            results,
                        )
                        .record(is_intersection);
                    }
                    walk_property_narrowing_expr(then_expr, ctx, results, floor, is_intersection);
                    return;
                }
            }
            let else_span = cond.r#else.span();
            if ctx.cursor_offset >= else_span.start.offset
                && ctx.cursor_offset <= else_span.end.offset
            {
                if !condition_is_stale {
                    narrowing::try_apply_instanceof_narrowing_inverse(
                        cond.condition,
                        else_span,
                        ctx,
                        results,
                    )
                    .record(is_intersection);
                }
                walk_property_narrowing_expr(cond.r#else, ctx, results, floor, is_intersection);
            }
        }
        Expression::Assignment(assign) => {
            walk_property_narrowing_expr(assign.rhs, ctx, results, floor, is_intersection);
        }
        Expression::Binary(bin) => {
            walk_property_narrowing_expr(bin.lhs, ctx, results, floor, is_intersection);
            walk_property_narrowing_expr(bin.rhs, ctx, results, floor, is_intersection);
        }
        Expression::Parenthesized(inner) => {
            walk_property_narrowing_expr(inner.expression, ctx, results, floor, is_intersection);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                let arg_expr = crate::type_engine::types::narrowing::argument_value(arg);
                walk_property_narrowing_expr(arg_expr, ctx, results, floor, is_intersection);
            }
        }
        _ => {}
    }
}

/// Report whether a check no longer describes the subject at the cursor
/// because a write listed by [`stale_narrowing_floor`] came after it.
fn check_is_stale(condition: &mago_syntax::cst::Expression<'_>, floor: Option<u32>) -> bool {
    use mago_span::HasSpan;

    floor.is_some_and(|write_offset| condition.span().end.offset < write_offset)
}

/// Offset of the last write before the cursor that replaces the value the
/// subject path is read from.
///
/// A check only describes the object its subject held when it ran.
/// Writing to a path the subject is rooted at (`$a = …`, or `$a->b = …`
/// for the subject `$a->b->c`) swaps that object out, so every check
/// written before the write describes something that is no longer there.
/// The forward walker says the same thing by dropping every scope key
/// rooted at the written path; this walk has no scope to drop, so it
/// records where the write happened and ignores the checks it invalidates.
fn stale_narrowing_floor<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::cst::Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<u32> {
    let subject = ctx.var_name;
    // The root is what the rest of the path is read from.  A subject
    // that is just a variable has no root to lose, so it needs no walk;
    // a static property's root (`self`, `Foo`) is never a `$`-prefixed
    // expression writes can target, so it needs none either.
    let root_len = subject
        .find("->")
        .into_iter()
        .chain(subject.find('['))
        .min()?;
    let root = &subject[..root_len];
    if !root.starts_with('$') {
        return None;
    }

    let mut floor = None;
    scan_stmts_for_writes(statements, subject, ctx.cursor_offset, &mut floor);
    floor
}

/// Record the offset of every write to an ancestor path of `subject`
/// that completes before `cursor`, keeping the last one.
fn scan_stmts_for_writes<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::cst::Statement<'b>>,
    subject: &str,
    cursor: u32,
    floor: &mut Option<u32>,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    for stmt in statements {
        if stmt.span().start.offset >= cursor {
            continue;
        }

        match stmt {
            Statement::Expression(expr_stmt) => {
                scan_expr_for_writes(expr_stmt.expression, subject, cursor, floor);
            }
            Statement::Return(ret) => {
                if let Some(value) = ret.value {
                    scan_expr_for_writes(value, subject, cursor, floor);
                }
            }
            Statement::If(if_stmt) => {
                scan_expr_for_writes(if_stmt.condition, subject, cursor, floor);
                scan_stmts_for_writes(if_stmt.body.statements().iter(), subject, cursor, floor);
                for (condition, statements) in if_stmt.body.else_if_clauses() {
                    scan_expr_for_writes(condition, subject, cursor, floor);
                    scan_stmts_for_writes(statements.iter(), subject, cursor, floor);
                }
                if let Some(statements) = if_stmt.body.else_statements() {
                    scan_stmts_for_writes(statements.iter(), subject, cursor, floor);
                }
            }
            Statement::Block(block) => {
                scan_stmts_for_writes(block.statements.iter(), subject, cursor, floor);
            }
            Statement::Foreach(foreach) => {
                // `foreach (… as $a)` rebinds `$a` on every iteration.
                match &foreach.target {
                    ForeachTarget::Value(val) => {
                        note_write(val.value, subject, cursor, floor);
                    }
                    ForeachTarget::KeyValue(kv) => {
                        note_write(kv.key, subject, cursor, floor);
                        note_write(kv.value, subject, cursor, floor);
                    }
                }
                scan_expr_for_writes(foreach.expression, subject, cursor, floor);
                scan_stmts_for_writes(foreach.body.statements().iter(), subject, cursor, floor);
            }
            Statement::While(while_stmt) => {
                scan_expr_for_writes(while_stmt.condition, subject, cursor, floor);
                scan_stmts_for_writes(while_stmt.body.statements().iter(), subject, cursor, floor);
            }
            Statement::For(for_stmt) => {
                for init in for_stmt.initializations.iter() {
                    scan_expr_for_writes(init, subject, cursor, floor);
                }
                for cond in for_stmt.conditions.iter() {
                    scan_expr_for_writes(cond, subject, cursor, floor);
                }
                for increment in for_stmt.increments.iter() {
                    scan_expr_for_writes(increment, subject, cursor, floor);
                }
                scan_stmts_for_writes(for_stmt.body.statements().iter(), subject, cursor, floor);
            }
            Statement::DoWhile(dw) => {
                scan_stmt_for_writes(dw.statement, subject, cursor, floor);
                scan_expr_for_writes(dw.condition, subject, cursor, floor);
            }
            Statement::Try(try_stmt) => {
                scan_stmts_for_writes(try_stmt.block.statements.iter(), subject, cursor, floor);
                for catch in try_stmt.catch_clauses.iter() {
                    scan_stmts_for_writes(catch.block.statements.iter(), subject, cursor, floor);
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    scan_stmts_for_writes(finally.block.statements.iter(), subject, cursor, floor);
                }
            }
            Statement::Switch(switch) => {
                scan_expr_for_writes(switch.expression, subject, cursor, floor);
                for case in switch.body.cases().iter() {
                    scan_stmts_for_writes(case.statements().iter(), subject, cursor, floor);
                }
            }
            _ => {}
        }
    }
}

/// Dispatch a single statement to [`scan_stmts_for_writes`].
fn scan_stmt_for_writes(
    stmt: &mago_syntax::cst::Statement<'_>,
    subject: &str,
    cursor: u32,
    floor: &mut Option<u32>,
) {
    scan_stmts_for_writes(std::iter::once(stmt), subject, cursor, floor);
}

/// Find assignments nested inside an expression.  Closures are left
/// alone: their bodies carry no narrowing for this walk either, and a
/// by-value capture cannot change what the enclosing scope holds.
fn scan_expr_for_writes(
    expr: &mago_syntax::cst::Expression<'_>,
    subject: &str,
    cursor: u32,
    floor: &mut Option<u32>,
) {
    use mago_syntax::cst::*;

    match expr {
        Expression::Assignment(assign) => {
            note_write(assign.lhs, subject, cursor, floor);
            scan_expr_for_writes(assign.rhs, subject, cursor, floor);
        }
        Expression::Binary(bin) => {
            scan_expr_for_writes(bin.lhs, subject, cursor, floor);
            scan_expr_for_writes(bin.rhs, subject, cursor, floor);
        }
        Expression::Parenthesized(inner) => {
            scan_expr_for_writes(inner.expression, subject, cursor, floor);
        }
        Expression::UnaryPrefix(unary) => {
            scan_expr_for_writes(unary.operand, subject, cursor, floor);
        }
        Expression::Conditional(cond) => {
            scan_expr_for_writes(cond.condition, subject, cursor, floor);
            if let Some(then_expr) = cond.then {
                scan_expr_for_writes(then_expr, subject, cursor, floor);
            }
            scan_expr_for_writes(cond.r#else, subject, cursor, floor);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                let arg_expr = crate::type_engine::types::narrowing::argument_value(arg);
                scan_expr_for_writes(arg_expr, subject, cursor, floor);
            }
        }
        _ => {}
    }
}

/// Keep `target`'s offset when writing to it replaces the value the
/// subject path is read from — either an ancestor of the subject, or
/// the subject itself.  A self-write's own type is resolved elsewhere
/// (the forward walker's scope), so this walk only needs to know that
/// a check preceding the write no longer describes what the subject
/// holds; it does not need the write's new type.
fn note_write(
    target: &mago_syntax::cst::Expression<'_>,
    subject: &str,
    cursor: u32,
    floor: &mut Option<u32>,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    // `foreach (… as &$a)` and `$a = &$b` wrap the target in a reference.
    let target = match target {
        Expression::UnaryPrefix(unary)
            if matches!(unary.operator, unary::UnaryPrefixOperator::Reference(_)) =>
        {
            unary.operand
        }
        other => other,
    };

    let span = target.span();
    // A write the cursor sits inside has not happened yet: the read on
    // its right-hand side still sees the old value.
    if span.end.offset >= cursor {
        return;
    }

    let Some(key) = crate::type_engine::types::narrowing::expr_to_subject_key(target) else {
        return;
    };
    // An ancestor of the subject invalidates it (`$obj = …` stales
    // `$obj->prop`), and so does a write to the exact subject
    // (`$obj->prop = …` stales a check made about the old value).
    let Some(rest) = subject.strip_prefix(key.as_str()) else {
        return;
    };
    if !rest.is_empty() && !rest.starts_with("->") && !rest.starts_with('[') {
        return;
    }

    let offset = span.start.offset;
    if floor.is_none_or(|current| current < offset) {
        *floor = Some(offset);
    }
}
