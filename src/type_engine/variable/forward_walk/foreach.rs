//! The `foreach` machinery: walking the statement, what the iterated
//! expression yields, how the key and value targets are bound from it, and
//! what the walker has to forget about the target variables on re-entry.

use super::*;
use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::types::narrowing;
use crate::type_engine::variable::foreach_resolution::{
    is_unsubstituted_template_param, resolve_iterable_element_via_class,
};
use crate::types::ResolvedType;

/// Record the dependency a `foreach` header creates: every variable the
/// target binds takes its type from the iterated expression.
///
/// Without this edge a loop that destructures an array it also writes to
/// looks dependency-free, so the fixed-point walk stops before the
/// element type it wrote has been read back.
pub(crate) fn collect_foreach_header_deps(
    foreach: &Foreach<'_>,
    deps: &mut HashMap<String, HashSet<String>>,
) {
    let mut iter_vars = HashSet::new();
    collect_rhs_variables(foreach.expression, &mut iter_vars);
    if iter_vars.is_empty() {
        return;
    }

    let mut bound = HashSet::new();
    match &foreach.target {
        ForeachTarget::Value(val) => collect_foreach_bound_vars(val.value, &mut bound),
        ForeachTarget::KeyValue(kv) => {
            collect_foreach_bound_vars(kv.key, &mut bound);
            collect_foreach_bound_vars(kv.value, &mut bound);
        }
    }

    for name in bound {
        deps.entry(name)
            .or_default()
            .extend(iter_vars.iter().cloned());
    }
}

/// Collect the variables a `foreach` target binds, unwrapping `&$v` and
/// recursing through destructuring patterns.
fn collect_foreach_bound_vars(target: &Expression<'_>, out: &mut HashSet<String>) {
    let target = if let Expression::UnaryPrefix(up) = target
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        target
    };
    collect_assignment_target_vars(target, out);
}

/// Narrow the collection a loop iterated to what the loop proved about
/// every one of its entries.
///
/// `foreach ($conds as $cond) { if (!$cond instanceof C) { break 2; } … }`
/// only falls out of its own bottom once every entry has passed the guard,
/// so the code after it may treat the whole collection as `C[]` — which is
/// what a second loop over the same expression, the idiom this exists for,
/// then reads its own variable from.  An empty collection makes the claim
/// vacuously true, so whether the body ran does not matter.
///
/// A `break` or `continue` naming only this loop proves nothing: the first
/// jumps straight to the code being narrowed, and the second skips the
/// entry rather than the rest of the program.
pub(crate) fn narrow_iterated_collection<'b>(
    foreach: &'b Foreach<'b>,
    body_stmts: &[&'b Statement<'b>],
    iter_type: Option<&PhpType>,
    entry_value_types: Option<&[ResolvedType]>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<()> {
    // A braced body arrives as a single `Block` statement, and the guards
    // are its children rather than the body's.  Checked before anything is
    // allocated: a loop that does not open with a guard is the common case
    // and there is nothing here for it.
    let leading = match body_stmts {
        [Statement::Block(block)] => block.statements.first(),
        _ => body_stmts.first().copied(),
    };
    guard_past_loop_condition(leading?)?;

    let entry_value_types = entry_value_types.filter(|types| !types.is_empty())?;
    let iter_type = iter_type?;
    let value_expr = match &foreach.target {
        ForeachTarget::Value(val) => val.value,
        ForeachTarget::KeyValue(kv) => kv.value,
    };
    // A by-reference loop writes through the entries it visits, so what a
    // guard proved about one need not still hold afterwards.
    if let Expression::UnaryPrefix(up) = value_expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        return None;
    }
    let Expression::Variable(Variable::Direct(dv)) = value_expr else {
        return None;
    };
    let var_name = bytes_to_str(dv.name).to_string();
    let collection_key = narrowing::expr_to_subject_key(foreach.expression)?;

    // Replay the leading guards against the entry binding alone: what
    // survives all of them is what every entry had to be to get here.
    let mut guard_scope = ScopeState::new();
    guard_scope.set(&var_name, entry_value_types.to_vec());
    let unwrapped: Vec<&Statement<'_>> = match body_stmts {
        [Statement::Block(block)] => block.statements.iter().collect(),
        _ => body_stmts.to_vec(),
    };
    for stmt in &unwrapped {
        let Some(condition) = guard_past_loop_condition(stmt) else {
            break;
        };
        apply_condition_narrowing_inverse(condition, &mut guard_scope, ctx);
        if guard_scope.unreachable {
            return None;
        }
    }

    let narrowed = guard_scope.get(&var_name);
    if narrowed.is_empty() || !narrowing_changed_types(entry_value_types, narrowed) {
        return None;
    }
    let element = ResolvedType::types_joined(narrowed);
    let collection_type =
        crate::type_engine::variable::array_func_rules::with_element_type(iter_type, element)?;
    // Keep whatever class backs the container itself (a `Collection` object
    // rather than a plain array); only its element type changed.
    let mut entry = scope
        .get(&collection_key)
        .first()
        .cloned()
        .unwrap_or_else(|| ResolvedType::from_type_string(collection_type.clone()));
    entry.type_string = collection_type;
    scope.set(&collection_key, vec![entry]);
    Some(())
}

/// The unexpanded iterable type, tried source by source.
pub(crate) fn resolve_foreach_iterable_type_raw<'b>(
    foreach: &'b Foreach<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // Try direct scope lookup for bare variable iterators.
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        let var_name = bytes_to_str(dv.name).to_string();
        let from_scope = scope.get(&var_name);
        if !from_scope.is_empty() {
            return Some(ResolvedType::types_joined(from_scope));
        }
    }

    // Fall back to resolve_rhs_expression for complex expressions.
    let resolved = resolve_rhs_with_scope(foreach.expression, scope, ctx);
    if !resolved.is_empty() {
        return Some(ResolvedType::types_joined(&resolved));
    }

    // Fallback: for simple `$variable` iterators, check for an inline
    // `/** @var Type $var */` or `@param` annotation near the foreach.
    // Handles cases where the variable's type comes from a docblock
    // rather than an assignment.
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        let var_name = bytes_to_str(dv.name).to_string();
        let foreach_offset = foreach.foreach.span().start.offset as usize;
        if let Some(docblock_type) = crate::docblock::find_iterable_raw_type_in_source(
            ctx.content,
            foreach_offset,
            &var_name,
        )
        .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
        {
            return Some(docblock_type);
        }
    }

    // Final fallback: resolve the foreach expression as a "subject"
    // through the full resolver pipeline (SubjectExpr::parse →
    // property/method chain resolution).  Handles cases like
    // `$this->getItems()` or `self::fetchAll()` where the expression
    // type wasn't captured by scope lookup or resolve_rhs_expression
    // above.
    if let Some(iter_type) = resolve_foreach_expr_via_subject(foreach.expression, scope, ctx) {
        return Some(iter_type);
    }

    None
}

/// The pieces of a forward-walk context the shared iterable element/key
/// derivation reads.
pub(crate) fn iterable_ctx<'a>(
    ctx: &'a ForwardWalkCtx<'_>,
) -> crate::type_engine::variable::foreach_resolution::IterableCtx<'a> {
    crate::type_engine::variable::foreach_resolution::IterableCtx {
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        class_loader: ctx.class_loader,
        resolved_class_cache: ctx.resolved_class_cache,
    }
}

/// Undo what the previous iteration wrote to a `foreach` target
/// variable, ahead of re-binding it for the next one.
///
/// The loop hands the target a fresh element at the top of every
/// iteration, so a write in the body — `$step = …`, and just as much
/// `$step['fo'] = …` — describes the element that iteration was given,
/// not the next one.  Merging it back over the loop's back edge leaves
/// the rebound variable carrying a type it cannot have, which then
/// defeats the guards in the body that would have narrowed it.
///
/// What the variable held *before* the loop is put back rather than
/// dropped: a `foreach` whose element type nothing can settle leaves the
/// name where it found it, and a loop that shadows an outer variable of
/// the same name is then no worse off than it was before the loop.
/// Clearing the entry first also drops the synthetic `$step['fo']` keys
/// and the proofs recorded against them, which the rebinding invalidates
/// whether or not a type replaces them.
pub(crate) fn reset_foreach_target(
    expr: &Expression<'_>,
    scope: &mut ScopeState,
    pre_loop_scope: &ScopeState,
) {
    let inner = if let Expression::UnaryPrefix(up) = expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        expr
    };
    match inner {
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name);
            scope.remove(var_name);
            scope.invalidate_dependent_keys(var_name);
            if pre_loop_scope.contains(var_name) {
                let before = pre_loop_scope.get(var_name);
                if before.is_empty() {
                    scope.set_empty(var_name);
                } else {
                    scope.set(var_name, before.to_vec());
                }
            }
        }
        // Destructuring targets: `foreach ($rows as [$a, $b])` binds
        // every variable in the pattern, each one just as fresh.
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                reset_foreach_destructured_element(elem, scope, pre_loop_scope);
            }
        }
        Expression::List(list) => {
            for elem in list.elements.iter() {
                reset_foreach_destructured_element(elem, scope, pre_loop_scope);
            }
        }
        _ => {}
    }
}

fn reset_foreach_destructured_element(
    elem: &ArrayElement<'_>,
    scope: &mut ScopeState,
    pre_loop_scope: &ScopeState,
) {
    match elem {
        ArrayElement::KeyValue(kv) => reset_foreach_target(kv.value, scope, pre_loop_scope),
        ArrayElement::Value(val) => reset_foreach_target(val.value, scope, pre_loop_scope),
        _ => {}
    }
}

/// Process a `foreach` statement.
pub(crate) fn process_foreach<'b>(
    foreach: &'b Foreach<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let depth_guard = LoopDepthGuard::enter();
    let loop_depth = depth_guard.depth();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        return;
    }

    // Apply any standalone `/** @var Type $var */` docblocks that precede
    // the foreach keyword.  These are not separate AST statements (the
    // parser attaches them as comments to the foreach), so they won't be
    // processed by `process_expression_statement`.  Without this, variables
    // typed only via docblock (common in Blade templates) won't be in scope
    // when the iterable expression is resolved.
    //
    // We extract all variables referenced in the foreach expression and
    // check for @var annotations for each one.
    let foreach_offset = foreach.foreach.span().start.offset as usize;
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        // `bytes_to_str(dv.name)` already includes the leading `$`, which
        // is how scope keys and `find_var_raw_type_in_source` expect it.
        let var_name = bytes_to_str(dv.name);
        if let Some(var_type) =
            crate::docblock::find_var_raw_type_in_source(ctx.content, foreach_offset, var_name)
        {
            let php_type = crate::util::resolve_php_type_names(&var_type, ctx.class_loader);
            // An explicit inline `@var` seeds an empty scope entry, and it
            // also refines a non-informative pre-existing type such as a
            // `mixed` closure/function parameter or a bare `array`.  Without
            // the second case, a `mixed` parameter would occupy the scope
            // slot and shadow the developer's `@var iterable<T> $x`
            // annotation, leaving the loop variable untyped.
            let current = scope.get(var_name);
            let should_apply = current.is_empty()
                || current.iter().all(|rt| {
                    crate::docblock::should_override_type_typed(&php_type, &rt.type_string)
                });
            if should_apply {
                let resolved = resolve_type_to_resolved_types(&php_type, ctx);
                scope.set(var_name, resolved);
            }
        }
    } else {
        // For complex expressions like `$users->active()->byName()`,
        // extract the base variable and resolve its type from @var.
        let expr_start = foreach.expression.span().start.offset as usize;
        let expr_end = foreach.expression.span().end.offset as usize;
        if let Some(expr_text) = ctx.content.get(expr_start..expr_end) {
            // Extract the base variable (e.g. "$users" from "$users->active()->byName()")
            if let Some(base_end) = expr_text.find("->").or_else(|| expr_text.find("::")) {
                let base_var = expr_text[..base_end].trim();
                // Scope keys retain the leading `$` (e.g. "$users"), so the
                // lookup and the insert must both use the `$`-prefixed name,
                // matching the direct-variable branch above.
                if base_var.starts_with('$')
                    && let Some(var_type) = crate::docblock::find_var_raw_type_in_source(
                        ctx.content,
                        foreach_offset,
                        base_var,
                    )
                {
                    let php_type = crate::util::resolve_php_type_names(&var_type, ctx.class_loader);
                    // As in the direct-variable branch: seed an unknown base
                    // variable, or refine a non-informative pre-existing type
                    // (e.g. a `mixed` parameter), but never clobber a more
                    // precise type inferred from an assignment.
                    let current = scope.get(base_var);
                    let should_apply = current.is_empty()
                        || current.iter().all(|rt| {
                            crate::docblock::should_override_type_typed(&php_type, &rt.type_string)
                        });
                    if should_apply {
                        let resolved = resolve_type_to_resolved_types(&php_type, ctx);
                        scope.set(base_var, resolved);
                    }
                }
            }
        }
    }

    // The iterable expression is an expression position like any other,
    // so the narrowing its own short-circuit chains, ternary branches and
    // `match (true)` arms prove has to reach the code inside them.
    // `foreach ($t instanceof UnionType ? $t->getTypes() : [$t] as $inner)`
    // reads `getTypes()` off the narrowed subject, not the declared one.
    record_short_circuit_snapshots(foreach.expression, scope, ctx);
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(foreach.expression, scope, ctx);
    }

    // Resolve the iterable expression's type.
    let iter_type = resolve_foreach_iterable_type(foreach, scope, ctx);

    let pre_loop_scope = scope.clone();

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &foreach.body {
        ForeachBody::Statement(inner) => inner.span(),
        ForeachBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = (if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    })
    .with_in_loop(true);
    let loop_body_ctx = ctx.with_in_loop(true);

    // Bind the value variable (and optionally the key variable).
    match &foreach.target {
        ForeachTarget::Value(val) => {
            bind_foreach_value(val.value, &iter_type, scope, ctx);
        }
        ForeachTarget::KeyValue(kv) => {
            bind_foreach_key(kv.key, &iter_type, scope, ctx);
            bind_foreach_value(kv.value, &iter_type, scope, ctx);
        }
    }

    // Docblock fallback: when `bind_foreach_value`/`bind_foreach_key`
    // could not determine the element type from the iterable (e.g. the
    // iterable is `mixed` or a bare `array`), check for inline
    // `/** @var Type $var */` docblock(s) preceding the foreach keyword
    // and use them to seed the key and/or value variables.  @var
    // annotations are explicit developer overrides that take priority
    // over types inferred from the iterable.
    let value_var_name = match &foreach.target {
        ForeachTarget::Value(val) => extract_foreach_var_name(val.value),
        ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.value),
    };
    let key_var_name = match &foreach.target {
        ForeachTarget::Value(_) => None,
        ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.key),
    };

    // Collect resolved docblock overrides for key/value variables.
    let mut value_docblock_override: Option<Vec<ResolvedType>> = None;
    let mut key_docblock_override: Option<Vec<ResolvedType>> = None;
    let foreach_offset = foreach.foreach.span().start.offset as usize;
    let before = &ctx.content[..foreach_offset.min(ctx.content.len())];
    let trimmed = before.trim_end();
    if trimmed.ends_with("*/")
        && let Some(doc_start) = trimmed.rfind("/**")
    {
        let doc_text = &trimmed[doc_start..trimmed.len()];
        let var_annotations = parse_var_docblock_pairs(doc_text);
        for (doc_var, php_type) in &var_annotations {
            if let Some(ref vn) = value_var_name
                && doc_var == vn
            {
                value_docblock_override = Some(resolve_type_to_resolved_types(php_type, ctx));
            }
            if let Some(ref kn) = key_var_name
                && doc_var == kn
            {
                key_docblock_override = Some(resolve_type_to_resolved_types(php_type, ctx));
            }
        }
    }

    // Apply docblock overrides (overwrites bind_foreach_key/value results).
    if let Some(ref resolved) = value_docblock_override
        && let Some(ref vn) = value_var_name
    {
        scope.set(vn, resolved.clone());
    }
    if let Some(ref resolved) = key_docblock_override
        && let Some(ref kn) = key_var_name
    {
        scope.set(kn, resolved.clone());
    }
    if value_docblock_override.is_none() {
        record_key_value_pairing(foreach, iter_type.as_ref(), scope, ctx);
    }
    // When the iterable is a bare `array` (no generic parameters)
    // and no @var docblock provided a concrete type, the element
    // type is `mixed`.  Seed it so that assignments from the loop
    // variable propagate `mixed` correctly through the body.
    if let Some(ref vn) = value_var_name
        && value_docblock_override.is_none()
        && scope.get(vn).is_empty()
        && iter_type.as_ref().is_some_and(|it| it.is_bare_array())
    {
        scope.set(vn, vec![ResolvedType::from_type_string(PhpType::mixed())]);
    }

    // What one entry looks like before the body has said anything about
    // it, which is the baseline the loop's own guards narrow.
    let entry_value_types: Option<Vec<ResolvedType>> =
        value_var_name.as_ref().map(|vn| scope.get(vn).to_vec());

    // ── Assignment-depth-bounded loop iteration ─────────────────
    //
    // Walk the body once (always needed).  Then check whether any
    // variable types changed compared to the pre-loop scope.  Only
    // re-walk if there are actual changes AND the assignment depth
    // requires further propagation.  This matches Mago's approach:
    // the fixed-point check happens BEFORE the expensive re-walk,
    // not after.
    let body_stmts: Vec<&Statement<'b>> = match &foreach.body {
        ForeachBody::Statement(inner) => vec![*inner],
        ForeachBody::ColonDelimited(body) => body.statements.iter().collect(),
    };
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // A loop that walks an array's own keys and unconditionally writes the
    // entry at each key it visits (`foreach ($pairs as $cn => $_)` /
    // `foreach (array_keys($pairs) as $cn)` with `$pairs[$cn][...] = …` in
    // the body) rewrites every entry the array has, whether it has one or
    // a thousand.  An empty array has no entries to leave behind, so the
    // claim holds vacuously when the loop does not run at all — see
    // `own_key_write_element` below for where this is applied.
    let own_key_write_target = foreach_own_keys(foreach)
        .filter(|(array_var, key_var)| body_writes_own_key(&body_stmts, array_var, key_var));

    // A `foreach` over an array the engine watched being built and knows
    // is still empty runs zero times, so it cannot change any type.  The
    // body is still walked so that a cursor or diagnostic inside it is
    // answered, but its writes are dropped afterwards.
    //
    // Keeping them would poison an enclosing loop's fixed point: the
    // first walk of an outer loop reaches an inner `foreach` over the
    // accumulator before anything has been written to it, and the
    // unresolved element types that walk produces would be unioned into
    // the accumulator for good, so the element type never converges.
    if iter_type
        .as_ref()
        .is_some_and(|it| it.is_empty_array_shape())
    {
        let exit_frame = ExitFrameGuard::push();
        walk_body_forward(body_stmts.iter().copied(), scope, &loop_body_ctx);
        exit_frame.pop();
        *scope = pre_loop_scope;
        return;
    }

    let exit_frame = ExitFrameGuard::push();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx: &loop_body_ctx,
            discovery_ctx: &discovery_ctx,
        },
        |next_scope, point| {
            if point != LoopSeedPoint::Entry {
                return;
            }
            // Re-bind the foreach variables for the next iteration,
            // discarding what the previous one wrote to them.
            match &foreach.target {
                ForeachTarget::Value(val) => {
                    reset_foreach_target(val.value, next_scope, &pre_loop_scope);
                    bind_foreach_value(val.value, &iter_type, next_scope, ctx);
                }
                ForeachTarget::KeyValue(kv) => {
                    reset_foreach_target(kv.key, next_scope, &pre_loop_scope);
                    reset_foreach_target(kv.value, next_scope, &pre_loop_scope);
                    bind_foreach_key(kv.key, &iter_type, next_scope, ctx);
                    bind_foreach_value(kv.value, &iter_type, next_scope, ctx);
                }
            }
            // Re-apply docblock overrides after re-binding.
            if let Some(ref resolved) = value_docblock_override
                && let Some(ref vn) = value_var_name
            {
                next_scope.set(vn, resolved.clone());
            }
            if let Some(ref resolved) = key_docblock_override
                && let Some(ref kn) = key_var_name
            {
                next_scope.set(kn, resolved.clone());
            }
            if value_docblock_override.is_none() {
                record_key_value_pairing(foreach, iter_type.as_ref(), next_scope, ctx);
            }
        },
    );

    let exits = exit_frame.pop();

    // Snapshot the array's freshly-written type here, while `scope` still
    // holds the raw post-body state: it is what every entry looks like
    // after the write the loop applies to the key it is visiting. A
    // `break` leaves some entries unvisited (and so unwritten), so the
    // rewrite-every-entry claim only holds when the loop always runs to
    // its own end.
    let own_key_write_element = own_key_write_target
        .as_ref()
        .filter(|_| exits.breaks.is_empty() && !scope.unreachable)
        .and_then(|(array_var, _)| {
            let written = scope.get(array_var);
            (!written.is_empty()).then(|| (array_var.clone(), written.to_vec()))
        });

    let written_element = if cursor_in_body {
        None
    } else {
        by_ref_written_element(foreach, entry_value_types.as_deref(), scope, &exits.breaks)
    };
    let iterated_before = written_element
        .as_ref()
        .map(|(name, _)| pre_loop_scope.get(name).to_vec());

    // An iterable that proves it has entries — a non-empty array literal,
    // or a type refined to `non-empty-array`/`non-empty-list`/a required
    // shape entry — runs the body at least once, so what was known before
    // the loop is not an alternative to what the body left behind.  The
    // pre-loop sentinel (`$max = null` ahead of a loop that always
    // assigns) would otherwise survive the whole loop.  A body that never
    // falls out of its own bottom has no fall-through state to keep, so
    // it still takes the merge.
    let body_always_runs = !scope.unreachable
        && (is_non_empty_array_literal(foreach.expression)
            || iter_type
                .as_ref()
                .is_some_and(PhpType::is_provably_non_empty));

    if !body_always_runs {
        // The iterable might be empty, so the loop body might not execute
        // at all.  Merge with the pre-loop scope.
        let post_loop = scope.clone();
        *scope = pre_loop_scope;
        scope.merge_branch(&post_loop);
    }

    // A path that broke out never reached the end of the body, so the
    // fall-through alone does not describe it.
    if !cursor_in_body {
        merge_exit_edges(scope, &exits.breaks);
        if let (Some((name, element)), Some(before)) = (written_element, iterated_before) {
            write_back_by_ref_element(&name, element, &before, scope);
        }
        if let Some((array_var, types)) = own_key_write_element {
            scope.set(&array_var, types);
        }
        let _ = narrow_iterated_collection(
            foreach,
            &body_stmts,
            iter_type.as_ref(),
            entry_value_types.as_deref(),
            scope,
            ctx,
        );
    }
}

/// Record what each key of an iterated shape pairs with, so narrowing the
/// key variable narrows the value it was read with.
///
/// ```php
/// /** @var array{psr-4?: array<string, string>, classmap?: list<string>} $data */
/// foreach ($data as $key => $value) {
///     if ($key === 'classmap') {
///         $value;        // list<string>
///         $data[$key];   // list<string>
///     }
/// }
/// ```
///
/// Each shape entry becomes a proof held by the key variable: once the key
/// is shown to be that entry's key, the value variable and the offset read
/// `$data[$key]` hold that entry's value type.  The entry cannot be missing
/// there, since the loop is visiting it.  Reassigning the key drops every
/// proof, and reassigning the value or the array drops the one about it
/// (see [`ScopeState::invalidate_proofs`]), so the pairing only lasts as
/// long as they still describe the same iteration.
fn record_key_value_pairing(
    foreach: &Foreach<'_>,
    iter_type: Option<&PhpType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let ForeachTarget::KeyValue(kv) = &foreach.target else {
        return;
    };
    let Expression::Variable(Variable::Direct(key_dv)) = kv.key else {
        return;
    };
    let Some(TypeKind::ArrayShape(entries)) = iter_type.map(PhpType::kind) else {
        return;
    };
    if entries.len() < 2 {
        return;
    }
    let key_var = bytes_to_str(key_dv.name);
    let value_var = extract_foreach_var_name(kv.value);
    let element_key =
        narrowing::expr_to_subject_key(foreach.expression).map(|base| format!("{base}[{key_var}]"));
    let targets: Vec<crate::atom::Atom> = value_var
        .iter()
        .map(|v| atom(v))
        .chain(element_key.iter().map(|k| atom(k)))
        .collect();
    if targets.is_empty() {
        return;
    }

    let mut proofs = Vec::with_capacity(entries.len() * targets.len());
    let mut position = 0usize;
    for entry in entries.iter() {
        let key = match entry.key.as_deref() {
            None => {
                position += 1;
                PhpType::literal_int((position - 1).to_string())
            }
            // Only known by its spelling, so no key comparison can match it.
            Some(key) if key.contains("::") => continue,
            Some(key) if crate::php_type::is_canonical_int_key(key) => PhpType::literal_int(key),
            Some(key) => PhpType::literal_string_value(key),
        };
        let trigger = vec![ResolvedType::from_type_string(key)];
        let types = ctx.resolved_types_for(entry.value_type.clone());
        for target in &targets {
            proofs.push(super::scope_state::ImpliedNarrowing {
                trigger: super::scope_state::ProofTrigger::Within(trigger.clone()),
                key: *target,
                types: types.clone(),
            });
        }
    }
    // The key variable was just rebound, so whatever it stood for on the
    // previous iteration is gone.
    scope.implied_narrowings.insert(atom(key_var), proofs);
}

/// The (array variable, key variable) a `foreach` binds when it walks an
/// array's own keys: `foreach ($arr as $key => $_)` or
/// `foreach (array_keys($arr) as $key)`.
///
/// Returns `None` for any other shape of iterable or target, including a
/// keyed loop over a *different* array than the one the key came from.
fn foreach_own_keys<'b>(foreach: &'b Foreach<'b>) -> Option<(String, String)> {
    if let ForeachTarget::KeyValue(kv) = &foreach.target
        && let Expression::Variable(Variable::Direct(arr_var)) = foreach.expression
        && let Expression::Variable(Variable::Direct(key_var)) = kv.key
    {
        return Some((
            bytes_to_str(arr_var.name).to_string(),
            bytes_to_str(key_var.name).to_string(),
        ));
    }

    let ForeachTarget::Value(val) = &foreach.target else {
        return None;
    };
    let Expression::Variable(Variable::Direct(key_var)) = val.value else {
        return None;
    };
    let Expression::Call(Call::Function(call)) = foreach.expression else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    if !crate::util::strip_fqn_prefix(bytes_to_str(ident.value()))
        .eq_ignore_ascii_case("array_keys")
    {
        return None;
    }
    let mut args = call.argument_list.arguments.iter();
    let arg = args.next()?;
    if args.next().is_some() {
        return None;
    }
    let Expression::Variable(Variable::Direct(arr_var)) = narrowing::argument_value(arg) else {
        return None;
    };
    Some((
        bytes_to_str(arr_var.name).to_string(),
        bytes_to_str(key_var.name).to_string(),
    ))
}

/// Whether the loop body unconditionally writes into `array_var[key_var]`
/// (optionally nested deeper, `array_var[key_var][...] = …`), the pattern
/// [`foreach_own_keys`] needs to guarantee every entry gets rewritten.
///
/// Only walks statements that always run when the body does (`Block`,
/// plain expression statements): a write nested inside an `if`/`switch`/
/// `try` may not touch every key, so it does not qualify.
fn body_writes_own_key(stmts: &[&Statement<'_>], array_var: &str, key_var: &str) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Statement::Block(block) => {
            let inner: Vec<&Statement<'_>> = block.statements.iter().collect();
            body_writes_own_key(&inner, array_var, key_var)
        }
        Statement::Expression(expr_stmt) => {
            assignment_targets_own_key(expr_stmt.expression, array_var, key_var)
        }
        _ => false,
    })
}

/// Whether `expr` is an assignment whose target is `array_var[key_var]`
/// (or a deeper access through it), as opposed to some unrelated key.
fn assignment_targets_own_key(expr: &Expression<'_>, array_var: &str, key_var: &str) -> bool {
    let Expression::Assignment(assign) = expr else {
        return false;
    };
    let outer_access = match assign.lhs {
        Expression::ArrayAccess(aa) => Some(aa),
        Expression::ArrayAppend(aa) => match aa.array {
            Expression::ArrayAccess(inner) => Some(inner),
            _ => None,
        },
        _ => None,
    };
    let Some(outer_access) = outer_access else {
        return false;
    };
    let Some((base_name, key_chain)) =
        super::super::array_shape_writes::extract_nested_array_access_chain(outer_access)
    else {
        return false;
    };
    if base_name != array_var {
        return false;
    }
    matches!(
        key_chain.first(),
        Some(Expression::Variable(Variable::Direct(dv))) if bytes_to_str(dv.name) == key_var
    )
}

/// The element type a by-reference `foreach` leaves in the array it
/// iterated, paired with that array's variable name.
///
/// `foreach ($list as &$value)` writes through `$value` into the entry it
/// is visiting, so every entry the loop finished with holds whatever
/// `$value` held at the end of the body (the fall-through and every
/// `continue`, which the walk has already joined into `scope`). A `break`
/// leaves the entry it stopped on with the value at that point and every
/// later entry untouched, so those alternatives join in too.
///
/// Returns `None` when the loop does not iterate a plain variable by
/// reference, or when the body leaves the entries as they were.
fn by_ref_written_element(
    foreach: &Foreach<'_>,
    entry_value_types: Option<&[ResolvedType]>,
    scope: &ScopeState,
    breaks: &[ScopeState],
) -> Option<(String, PhpType)> {
    let value_expr = match &foreach.target {
        ForeachTarget::Value(val) => val.value,
        ForeachTarget::KeyValue(kv) => kv.value,
    };
    let Expression::UnaryPrefix(up) = value_expr else {
        return None;
    };
    let (UnaryPrefixOperator::Reference(_), Expression::Variable(Variable::Direct(value_var))) =
        (&up.operator, up.operand)
    else {
        return None;
    };
    let Expression::Variable(Variable::Direct(iterated)) = foreach.expression else {
        return None;
    };
    let value_name = bytes_to_str(value_var.name);
    let entry = entry_value_types.filter(|types| !types.is_empty())?;

    let mut alternatives: Vec<ResolvedType> = Vec::new();
    if !scope.unreachable {
        alternatives.extend_from_slice(scope.get(value_name));
    }
    if !breaks.is_empty() {
        for edge in breaks {
            alternatives.extend_from_slice(edge.get(value_name));
        }
        alternatives.extend_from_slice(entry);
    }
    if alternatives.is_empty() {
        return None;
    }
    let element = ResolvedType::types_joined(&alternatives);
    if element.equivalent(&ResolvedType::types_joined(entry)) {
        return None;
    }
    Some((bytes_to_str(iterated.name).to_string(), element))
}

/// Give the array a by-reference `foreach` iterated the element type its
/// body wrote through the reference.
///
/// Only an array the body did not otherwise reassign is rewritten: once
/// the variable holds something other than what the loop started from,
/// the entries the reference wrote are no longer the ones it holds.
fn write_back_by_ref_element(
    name: &str,
    element: PhpType,
    before: &[ResolvedType],
    scope: &mut ScopeState,
) {
    let current = scope.get(name);
    if current.is_empty() || before.is_empty() {
        return;
    }
    let array_type = ResolvedType::types_joined(current);
    if !array_type.is_array_like() || array_type != ResolvedType::types_joined(before) {
        return;
    }
    let Some(rewritten) =
        crate::type_engine::variable::array_func_rules::with_element_type(&array_type, element)
    else {
        return;
    };
    let mut entry = current[0].clone();
    entry.type_string = rewritten;
    scope.set(name, vec![entry]);
}

/// Resolve the iterable expression's type for a foreach.
///
/// Every answer is run through `resolve_type_alias_typed` so a
/// `@phpstan-type` / `@phpstan-import-type` alias is expanded to the array
/// type it names before the caller reads a key or element type off it.
/// The expansion lives here rather than in each branch of
/// [`resolve_foreach_iterable_type_raw`] so a new branch cannot forget it.
pub(crate) fn resolve_foreach_iterable_type<'b>(
    foreach: &'b Foreach<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let raw = resolve_foreach_iterable_type_raw(foreach, scope, ctx)?;
    Some(
        crate::type_engine::type_resolution::resolve_type_alias_typed(
            &raw,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(raw),
    )
}

/// Resolve a foreach expression to a `PhpType` by treating it as a
/// subject string and going through the full resolver pipeline.
///
/// It extracts the expression text, calls `resolve_target_classes` to
/// get `ClassInfo` objects, and constructs a `TypeKind::Named` from the
/// first resolved class.
pub(crate) fn resolve_foreach_expr_via_subject<'b>(
    expression: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let expr_span = expression.span();
    let expr_start = expr_span.start.offset as usize;
    let expr_end = expr_span.end.offset as usize;
    let expr_text = ctx.content.get(expr_start..expr_end)?.trim();
    if expr_text.is_empty() {
        return None;
    }

    // Build a ResolutionCtx from the forward walker's context.
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = ctx.var_ctx_for_with_scope(
        "$__foreach",
        expr_span.start.offset,
        &scope_resolver,
        Some(scope.proofs()),
    );
    let rctx = var_ctx.as_resolution_ctx();

    let resolved = crate::type_engine::resolver::resolve_target_classes(
        expr_text,
        crate::types::AccessKind::Arrow,
        &rctx,
    );

    if resolved.is_empty() {
        return None;
    }

    // Construct a PhpType from the resolved classes.  If any resolved
    // type has a structured type_string (e.g. `list<User>`,
    // `Collection<int, Product>`), prefer that — it carries generic
    // parameters that `extract_value_type` can use.
    for rt in &resolved {
        if rt.type_string.has_type_structure() {
            let expanded = crate::type_engine::type_resolution::resolve_type_alias_typed(
                &rt.type_string,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
            .unwrap_or_else(|| rt.type_string.clone());
            return Some(expanded);
        }
    }

    // Fall back to the class name — `bind_foreach_value` Strategy 2
    // will resolve it through inheritance to find element types.
    // Use `fqn()` (not `name`) so that the returned `TypeKind::Named`
    // carries the fully-qualified class name.  `ClassInfo.name` is
    // always the short name (e.g. `OrderProductCollection`), while
    // `fqn()` combines namespace + name into the FQN that the class
    // loader needs to find and merge the class.
    let first = resolved.first()?;
    let name = first
        .class_info
        .as_ref()
        .map(|c| c.fqn().to_string())
        .or_else(|| first.type_string.base_name().map(|s| s.to_string()))?;

    Some(PhpType::named(atom(&name)))
}

/// Bind a foreach value variable from the iterable's element type.
///
/// Resolution strategy:
/// 1. Try `PhpType::extract_value_type` — works for types that already
///    carry generic parameters (e.g. `list<User>`, `array<int, Order>`,
///    `Collection<int, Product>`).
/// 2. Class-based fallback — when the type is a bare class name (e.g.
///    `OrderProductCollection`), resolve it to `ClassInfo`, merge
///    inheritance, and extract the element type from `@extends` /
///    `@implements` generics.
pub(crate) fn bind_foreach_value<'b>(
    value_expr: &'b Expression<'b>,
    iter_type: &Option<PhpType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Unwrap `&$value` (by-reference foreach) to get the inner variable.
    let value_expr = if let Expression::UnaryPrefix(up) = value_expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        value_expr
    };
    if let Expression::Variable(Variable::Direct(dv)) = value_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        if let Some(it) = iter_type {
            // Strategy 1: extract from the type's own generic parameters
            // (or, for tuple-style shapes, the union of positional values),
            // read through the class's traversal binding when it has one.
            let value_php_type =
                crate::type_engine::variable::foreach_resolution::generic_traversal_value_type(
                    it,
                    ctx.class_loader,
                )
                .or_else(|| it.iterable_element_type());
            if let Some(vt) = value_php_type {
                scope.set(&var_name, ctx.resolved_types_for(vt.clone()));
                return;
            }

            // Strategy 2: class-based fallback for bare collection names.
            let element_via_class = resolve_iterable_element_via_class(it, &iterable_ctx(ctx));
            if let Some(element_type) = element_via_class
                && !is_unsubstituted_template_param(&element_type)
            {
                scope.set(&var_name, ctx.resolved_types_for(element_type));
            }

            // Strategy 3: union type fallback — try each member individually.
            // When the iterable is a union like `ProductCollection|Product`,
            // neither `extract_value_type` nor `resolve_iterable_element_via_class`
            // works on the union as a whole.  Walk each member and use the
            // first one that yields an element type.
            if let TypeKind::Union(members) = it.kind() {
                for member in members {
                    // Try extract_value_type on each member (handles generic collections).
                    if let Some(vt) = member.extract_value_type(false) {
                        scope.set(&var_name, ctx.resolved_types_for(vt.clone()));
                        return;
                    }
                    // Try class-based element extraction on each member.
                    if let Some(element_type) =
                        resolve_iterable_element_via_class(member, &iterable_ctx(ctx))
                        && !is_unsubstituted_template_param(&element_type)
                    {
                        scope.set(&var_name, ctx.resolved_types_for(element_type));
                        return;
                    }
                }
            }
        }
        // Couldn't determine the element type (untyped/unknown iterable).
        // Seed `mixed` so body assignments like `$x = $value` after
        // `$x = null` overwrite pure-null and participate in post-loop
        // merge + `is_null` early-return narrowing.  Bare `array` is
        // already seeded as `mixed` above; fully untyped parameters
        // hit this path with `iter_type = None`.
        if scope.get(&var_name).is_empty() {
            scope.set(
                &var_name,
                vec![ResolvedType::from_type_string(PhpType::mixed())],
            );
        }
    } else if let Expression::Array(_) | Expression::List(_) = value_expr {
        // Array/list destructuring in foreach: `foreach ($items as [$a, $b])`
        // Extract the element type from the iterable, then resolve each
        // destructured variable's type from that element type using shape
        // keys or positional indices.
        let element_type: Option<PhpType> = iter_type.as_ref().and_then(|it| {
            crate::type_engine::variable::foreach_resolution::iteration_value_type(
                it,
                &iterable_ctx(ctx),
            )
        });

        if let Some(ref elem_type) = element_type {
            let elements_iter: Vec<&ArrayElement<'_>> = match value_expr {
                Expression::Array(arr) => arr.elements.iter().collect(),
                Expression::List(list) => list.elements.iter().collect(),
                _ => vec![],
            };

            let mut positional_index: usize = 0;
            for elem in elements_iter {
                let (var_name, shape_key) = match elem {
                    ArrayElement::KeyValue(kv) => {
                        if let Expression::Variable(Variable::Direct(dv)) = kv.value {
                            (
                                bytes_to_str(dv.name).to_string(),
                                extract_foreach_destr_key(kv.key),
                            )
                        } else {
                            continue;
                        }
                    }
                    ArrayElement::Value(val) => {
                        let key = Some(positional_index.to_string());
                        positional_index += 1;
                        if let Expression::Variable(Variable::Direct(dv)) = val.value {
                            (bytes_to_str(dv.name).to_string(), key)
                        } else {
                            continue;
                        }
                    }
                    // A hole (`foreach ($x as [, $parameter])`) names nothing
                    // but still consumes the position.
                    ArrayElement::Missing(_) => {
                        positional_index += 1;
                        continue;
                    }
                    _ => continue,
                };

                // Try shape key lookup first, then fall back to generic element type.
                let resolved_type = shape_key
                    .as_ref()
                    .and_then(|k| elem_type.shape_value_type(k).cloned())
                    .or_else(|| elem_type.extract_value_type(true).cloned());

                if let Some(vt) = resolved_type {
                    scope.set(&var_name, ctx.resolved_types_for(vt));
                }
            }
        }
    }
}

/// Returns `true` when `expr` is a non-empty array literal such as
/// `["a", "b", "c"]` or `array(1, 2, 3)`.
///
/// Used by `process_foreach` to detect iterables that are guaranteed to
/// have at least one element, so that the pre-loop type of the target
/// variable does not survive into the post-loop scope.
pub(crate) fn is_non_empty_array_literal(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Array(arr) => !arr.elements.is_empty(),
        Expression::LegacyArray(arr) => !arr.elements.is_empty(),
        _ => false,
    }
}

/// Extract the variable name from a foreach value expression, unwrapping
/// a leading `&` (by-reference) if present.
pub(crate) fn extract_foreach_var_name(expr: &Expression<'_>) -> Option<String> {
    let inner = if let Expression::UnaryPrefix(up) = expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        expr
    };
    if let Expression::Variable(Variable::Direct(dv)) = inner {
        Some(bytes_to_str(dv.name).to_string())
    } else {
        None
    }
}

/// Extract a string key from a foreach destructuring key expression.
///
/// Handles string literals (`'user'`, `"user"`) and integer literals.
pub(crate) fn extract_foreach_destr_key(key_expr: &Expression<'_>) -> Option<String> {
    match key_expr {
        Expression::Literal(Literal::String(lit_str)) => match lit_str.value {
            Some(bytes) => Some(literal_bytes_to_str(bytes)?.to_string()),
            None => {
                let raw = bytes_to_str(lit_str.raw).to_string();
                Some(raw.trim_matches('\'').trim_matches('"').to_string())
            }
        },
        Expression::Literal(Literal::Integer(lit_int)) => {
            Some(bytes_to_str(lit_int.raw).to_string())
        }
        _ => None,
    }
}

/// Bind a foreach key variable.
pub(crate) fn bind_foreach_key<'b>(
    key_expr: &'b Expression<'b>,
    iter_type: &Option<PhpType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Expression::Variable(Variable::Direct(dv)) = key_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        // A bare `array` says nothing about its keys, and neither does an
        // untyped iterable: both leave the key `int|string`.
        let key_type = iter_type.as_ref().and_then(|it| {
            crate::type_engine::variable::foreach_resolution::iteration_key_type(
                it,
                &iterable_ctx(ctx),
            )
        });
        // Benevolent, because `int|string` here is not something the array
        // said — it is the whole of PHP's key domain, standing in for a key
        // type nobody wrote down.  Holding the user to both branches of a
        // union we invented turns every `substr($key, …)` into a false
        // positive, so a single branch satisfies it (`is_type_compatible`
        // implements that half).
        let key_type = key_type.unwrap_or_else(|| {
            PhpType::benevolent(PhpType::union(vec![PhpType::int(), PhpType::string()]))
        });
        scope.set(&var_name, ctx.resolved_types_for(key_type));
    }
}
