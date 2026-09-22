//! The `foreach` machinery: what the iterated expression yields, and what
//! the walker has to forget about the target variables on re-entry.

use super::*;
use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::type_engine::types::narrowing;
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
