//! Reads of the forward walker's scope: what a variable holds, and the
//! narrowed type a check left under a member path or a call's own text.

use mago_span::HasSpan;
use mago_syntax::cst::Expression;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::variable::{forward_walk, resolution};
use crate::types::ResolvedType;

/// Resolve a variable's type for use in RHS expression evaluation.
///
/// When `ctx.scope_var_resolver` is set (forward-walker RHS
/// resolution), the scope resolver is consulted first.  This reads
/// directly from the forward walker's in-progress `ScopeState`,
/// avoiding re-entry into the forward walk.  Otherwise falls back to
/// [`resolution::resolve_variable_types`] (which itself checks the
/// diagnostic scope cache and then delegates to the forward walker).
pub(in crate::type_engine) fn resolve_var_types(
    var_name: &str,
    ctx: &VarResolutionCtx<'_>,
    cursor_offset: u32,
) -> Vec<ResolvedType> {
    // A narrowing established by the enclosing match arm or ternary
    // condition outranks the scope: it describes this position, while the
    // scope entry describes the variable before the condition was tested.
    if let Some(narrowed) = ctx.arm_narrowed(var_name) {
        return narrowed.clone();
    }

    // ── Forward-walker fast path ────────────────────────────────
    // When a scope_var_resolver is available, read variable types
    // directly from the forward walker's ScopeState.  This avoids
    // the feedback loop where the backward scanner hits the
    // (incomplete) diagnostic scope cache during the forward walk.
    if let Some(resolver) = ctx.scope_var_resolver {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        let from_scope = resolver(&prefixed);
        if !from_scope.is_empty() {
            return from_scope;
        }
        // The forward walker is the authority for variable types.
        // If the variable isn't in its ScopeState, it hasn't been
        // assigned yet at this point in the walk.  Falling through
        // to `resolve_variable_types` would re-enter the forward
        // walker, causing O(N²) blowup
        // or stack overflow.  Return empty so the RHS resolver
        // treats the variable as unresolved.
        return vec![];
    }

    resolution::resolve_variable_types(ctx.var_query(var_name, cursor_offset))
}

/// The narrowed type recorded for `key` at this expression's position,
/// if any.
///
/// An enclosing ternary arm or `match` arm comes first: it proved
/// something the surrounding statement's scope does not know, because the
/// check is part of the expression rather than a statement above it.  That
/// is what carries `getDocComment() !== false ? getDocComment() : null`
/// over to the repeated call in the true arm.
///
/// Failing that, the forward walker's scope answers: the live scope when
/// this resolution runs inside a walk, the snapshot the diagnostic walk
/// recorded at the expression, or, for a member path read by a request
/// made outside any walk (hover, completion), a walk to the request's
/// position.  Each is the same walk, so diagnostics, hover and completion
/// agree.
pub(super) fn narrowed_subject_from_scope(
    key: &str,
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if let Some(from_arm) = ctx.arm_narrowed(key)
        && !from_arm.is_empty()
    {
        return Some(from_arm.clone());
    }
    let from_scope = match ctx.scope_var_resolver {
        Some(resolver) => resolver(key),
        None if forward_walk::is_diagnostic_scope_active() => {
            if forward_walk::is_building_scopes() {
                return None;
            }
            forward_walk::lookup_diagnostic_scope(key, expr.span().start.offset)?
        }
        // Only a path read through `->` is worth a walk: the static and
        // offset keys a callee's body reads while its return type is
        // inferred would each walk that body again for every file that
        // calls it, and nothing narrows them often enough to pay for that.
        None if !key.contains("->") => return None,
        None => resolution::resolve_variable_types(ctx.var_query(key, ctx.cursor_offset)),
    };
    (!from_scope.is_empty()).then_some(from_scope)
}

/// The type a check on `call` narrowed it to, given what the method it
/// invokes declares it returns.
///
/// The narrowing is keyed under the call's own text, the same key the
/// subject-expression resolver builds, so the check and every later
/// occurrence of the call agree on what they are talking about.
pub(super) fn narrowed_call(
    call: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let key = crate::type_engine::types::narrowing::expr_to_subject_key(call)?;
    narrowed_subject_from_scope(&key, call, ctx)
}
