//! Right-hand-side expression resolution for variable assignments.
//!
//! This module resolves the type of the right-hand side of an assignment
//! (`$var = <expr>`) to zero or more [`ResolvedType`] values.  It handles:
//!
//!   - Scalar literals: `1` → `1`, `'hello'` → `'hello'`, etc.
//!   - Array literals: `[new Foo()]` → `list<Foo>`,
//!     `['a' => 1]` → `array{a: int}`
//!   - `new ClassName(…)` → the instantiated class
//!   - Array access: `$arr[0]` → generic element type,
//!     `$arr['key']` → array shape value type,
//!     `$arr['key'][0]` → chained bracket access
//!   - Function calls: `someFunc()` → return type
//!   - Method calls: `$this->method()`, `$obj->method()` → return type
//!   - Static calls: `ClassName::method()` → return type
//!   - Property access: `$this->prop`, `$obj->prop` → property type
//!   - Match expressions: union of all arm types
//!   - Ternary / null-coalescing: union of both branches
//!   - Clone: `clone $expr` → preserves the cloned expression's type
//!
//! The entry point is [`resolve_rhs_expression`], which dispatches to
//! specialised helpers based on the AST node kind.
//! It is shared by assignment tracking, diagnostics, callback inference,
//! and other expression consumers that need one canonical result.
//!
//! The dispatch logic lives here; specialised resolution is spread
//! across sibling files:
//!
//! - [`instantiation`]: `new ClassName(…)` and constructor template
//!   substitution.
//! - [`array_access`]: `$arr[0]` / `$arr['key']` generic element / shape
//!   value resolution.
//! - [`calls`]: the call dispatcher, function-level `@template`
//!   substitution, and the argument raw-type helpers template binding
//!   reads call arguments through.
//! - [`function_calls`], [`method_calls`], [`static_calls`]: return-type
//!   resolution for each call form, with [`owner_method_call`] resolving
//!   one owner class's method for both instance and static calls.
//! - [`laravel_calls`]: Laravel helpers and accessors whose arguments
//!   decide their return type.
//! - [`property_access`]: `$this->prop` / `$obj->prop` resolution and
//!   the `find_*_this_property_assignment*` scanners.
//! - [`unary_prefix`]: casts, `!`, `~`, and `++`/`--`.
//! - [`magic_constants`]: `__LINE__`, `__CLASS__`, `__METHOD__`, and the
//!   other magic constants.
//! - [`literal_text`]: types read from a constant initializer's or an
//!   array literal's source text.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{Atom, AtomMap, atom, bytes_to_str};
use crate::parser::extract_hint_type;
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::type_resolution;
use crate::util::strip_fqn_prefix;

mod arithmetic;
mod array_access;
mod calls;
mod function_calls;
mod instantiation;
mod laravel_calls;
mod literal_text;
mod magic_constants;
mod method_calls;
mod owner_method_call;
mod property_access;
mod scalar_fold;
mod static_calls;
mod unary_prefix;

pub(crate) use scalar_fold::fold_concat_types;

use arithmetic::resolve_binary_result_type;
use array_access::resolve_rhs_array_access;
use calls::resolve_rhs_call;
use instantiation::resolve_rhs_instantiation;
use magic_constants::magic_constant_type;
use method_calls::{MethodReceiver, resolve_method_call_on_receiver};
use property_access::resolve_rhs_property_access;
use unary_prefix::stepped_numeric_type;

pub(crate) use literal_text::{
    infer_array_literal_type, infer_type_from_constant_value,
    infer_type_from_constant_value_resolved,
};
pub(crate) use unary_prefix::unary_prefix_result_type;

pub(crate) use arithmetic::{
    ArithmeticOpKind, infer_addition_result_type, infer_arithmetic_result_type,
    infer_modulo_result_type,
};

pub(crate) use array_access::{class_string_inner_binding, insert_or_union, offset_read_type};
pub(crate) use calls::{
    ArgWalkerTypes, build_function_template_subs, infer_closure_literal_type,
    is_array_like_wrapper, resolve_arg_call_raw_type, resolve_arg_iterable_raw_type,
    resolve_arg_variable_raw_type, substitute_function_templates, walker_arg_types,
};
pub(crate) use instantiation::{
    TemplateBindingMode, array_element_binding, bound_binding_hint, candidate_binding_modes,
    class_string_generic_binding, classify_template_binding, extract_array_position,
    remap_inherited_ctor_subs, resolve_array_literal_generic, type_contains_name,
};

/// The type of a member access whose name PHP only works out at runtime.
///
/// `$obj->{$name}()`, `Cls::{$expr}()`, `$obj->{$name}`, `Cls::${$name}`
/// and `Cls::{$name}` all name a member no static lookup can find, so the
/// only honest answer is `mixed` — the type that admits every value.
/// Answering with nothing instead would say the type engine's resolution
/// fell short, sending whoever reads the diagnostic looking for a gap in
/// the engine rather than at code PHP itself leaves open.
pub(super) fn runtime_named_member_type() -> Vec<ResolvedType> {
    vec![ResolvedType::from_type_string(PhpType::mixed())]
}

/// Whether a member access's receiver can be `null`, as far as a
/// short-circuiting `?->` is concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiverNullability {
    Never,
    Maybe,
    Always,
}

impl ReceiverNullability {
    /// Read the nullability off a receiver's resolved types.  `mixed`
    /// counts as non-null: whatever a member of it reads is `mixed`
    /// already, so the extra `null` would say nothing.
    pub(super) fn of(resolved: &[ResolvedType]) -> Self {
        let mut any_null = false;
        let mut all_null = !resolved.is_empty();
        for rt in resolved {
            let ty = &rt.type_string;
            if ty.is_null() {
                any_null = true;
            } else {
                all_null = false;
                any_null |= matches!(ty.kind(), TypeKind::Nullable(_))
                    || matches!(ty.kind(), TypeKind::Union(members) if members.iter().any(PhpType::is_null));
            }
        }
        match (all_null, any_null) {
            (true, _) => Self::Always,
            (false, true) => Self::Maybe,
            (false, false) => Self::Never,
        }
    }
}

/// Whether a member access chain runs through a `?->` before reaching
/// `expr`'s own link, so that a `null` there short-circuits the rest of
/// the chain rather than being read through.
pub(super) fn chain_has_nullsafe<'b>(mut expr: &'b Expression<'b>) -> bool {
    loop {
        expr = peel_type_transparent(expr);
        expr = match expr {
            Expression::Access(Access::NullSafeProperty(_))
            | Expression::Call(Call::NullSafeMethod(_)) => return true,
            Expression::Access(Access::Property(pa)) => pa.object,
            Expression::Call(Call::Method(mc)) => mc.object,
            Expression::ArrayAccess(aa) => aa.array,
            _ => return false,
        };
    }
}

/// Fold a `?->` short-circuit into a member access's result.
///
/// A nullsafe link whose receiver is `null` evaluates to `null` without
/// reading the member, and so does every link after it in the same chain.
/// `nullsafe` is whether this link is `?->` itself; `object` is its
/// receiver expression, consulted only when the receiver can be `null`.
pub(super) fn apply_nullsafe_short_circuit(
    result: &mut Vec<ResolvedType>,
    receiver: ReceiverNullability,
    nullsafe: bool,
    object: &Expression<'_>,
) {
    if receiver == ReceiverNullability::Never || !(nullsafe || chain_has_nullsafe(object)) {
        return;
    }
    if receiver == ReceiverNullability::Always {
        *result = vec![ResolvedType::from_type_string(PhpType::null())];
        return;
    }
    if result.is_empty() || result.iter().any(|rt| rt.type_string.accepts_null()) {
        return;
    }
    // A lone entry carries the `null` in its own type, the way a `?Foo`
    // return does, so narrowing that rules the class in rules it out.
    if let [only] = result.as_mut_slice() {
        only.type_string = std::mem::replace(&mut only.type_string, PhpType::null()).or_null();
        return;
    }
    result.push(ResolvedType::from_type_string(PhpType::null()));
}

/// Apply unary `+` or `-` to an already-resolved numeric type.
///
/// Exact literal members remain exact; negating a literal `PHP_INT_MIN`
/// overflows and gives up rather than reporting a wrong value. A broad
/// integer stays an integer, matching PHPStan: modelling the one input that
/// overflows to a float would cost precision on every other negation.
/// Any non-numeric branch makes the result unknown.
fn apply_numeric_sign(ty: &PhpType, negated: bool) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Literal(value) => match &**value {
            LiteralValue::Int(_) => {
                let value = value.parse_i64()?;
                let value = if negated { value.checked_neg()? } else { value };
                Some(PhpType::literal_int(value.to_string()))
            }
            LiteralValue::Float(raw) => {
                let raw = raw.trim();
                let signed = if negated {
                    if let Some(rest) = raw.strip_prefix('-') {
                        rest.to_string()
                    } else if let Some(rest) = raw.strip_prefix('+') {
                        format!("-{rest}")
                    } else {
                        format!("-{raw}")
                    }
                } else {
                    raw.strip_prefix('+').unwrap_or(raw).to_string()
                };
                Some(PhpType::literal_float(signed))
            }
            // A numeric string reads as the number it spells (`-'1'` is
            // `-1`); any other string warns or throws.
            LiteralValue::String(_) => value
                .numeric_string_value()
                .and_then(|number| apply_numeric_sign(&PhpType::literal(number), negated)),
        },
        TypeKind::Union(members) => {
            let mut signed = Vec::with_capacity(members.len());
            for member in members {
                let member = apply_numeric_sign(member, negated)?;
                for alternative in member.union_members() {
                    if !signed.iter().any(|existing| existing == alternative) {
                        signed.push(alternative.clone());
                    }
                }
            }
            match signed.len() {
                0 => None,
                1 => signed.into_iter().next(),
                _ => Some(PhpType::union(signed)),
            }
        }
        // PHP coerces null here, but the exact result depends on runtime
        // version and diagnostics policy; keep the established conservative
        // fallback instead of silently discarding nullability.
        TypeKind::Nullable(_) => None,
        _ if ty.is_int_subtype() => Some(PhpType::int()),
        _ if ty.is_float_subtype() => Some(PhpType::float()),
        _ if ty.is_named_ci("numeric") || ty.is_named("number") => {
            Some(PhpType::union(vec![PhpType::int(), PhpType::float()]))
        }
        _ => None,
    }
}

/// Collapse only semantically redundant alternatives after joining control-flow
/// expression branches. Exact literal-only unions remain untouched.
fn simplify_branch_results(results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    let mut results = ResolvedType::collapse_redundant_runtime_literals(results);
    // `mixed_absorbs_siblings: false` — a ternary/match arm that only
    // resolves to `mixed` (an unresolved call) must not erase a sibling
    // arm's real, narrower answer; see `drop_subsumed_entries`.
    ResolvedType::drop_subsumed_entries(&mut results, false);
    results
}

/// PHP's runtime truthiness for a ternary condition, when it is a literal
/// whose truthiness is knowable at parse time.
///
/// Returns `None` for anything that isn't a bare scalar literal (a variable,
/// a call, a comparison, ...), so the caller falls back to unioning both arms.
pub(crate) fn static_condition_truthiness(expr: &Expression) -> Option<bool> {
    match expr {
        Expression::Parenthesized(inner) => static_condition_truthiness(inner.expression),
        Expression::Literal(Literal::True(_)) => Some(true),
        Expression::Literal(Literal::False(_)) => Some(false),
        Expression::Literal(Literal::Null(_)) => Some(false),
        Expression::Literal(Literal::Integer(integer)) => integer.value.map(|value| value != 0),
        Expression::Literal(Literal::Float(float)) => Some(float.value.into_inner() != 0.0),
        Expression::Literal(Literal::String(string)) => string.value.and_then(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .map(|value| !value.is_empty() && value != "0")
        }),
        _ => None,
    }
}

/// Resolve a variable's type for use in RHS expression evaluation.
///
/// When `ctx.scope_var_resolver` is set (forward-walker RHS
/// resolution), the scope resolver is consulted first.  This reads
/// directly from the forward walker's in-progress `ScopeState`,
/// avoiding re-entry into the forward walk.  Otherwise falls back to
/// [`resolve_variable_types`] (which itself checks the diagnostic
/// scope cache and then delegates to the forward walker).
fn resolve_var_types(
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

    super::resolution::resolve_variable_types(
        var_name,
        ctx.current_class,
        ctx.all_classes,
        ctx.content,
        cursor_offset,
        ctx.class_loader,
        ctx.source_class_loader,
        ctx.source_imports,
        ctx.backend,
        ctx.loaders,
    )
}

// ── Match-arm narrowing override ────────────────────────────────────
//
// When resolving the RHS of a `match(true)` arm like:
//
//   match (true) {
//       $model instanceof Customer => $model->country,
//       …
//   }
//
// the arm expression `$model->country` must resolve `$model` as
// `Customer`, not its declared parameter type `?Model`.  The normal
// variable resolution pipeline doesn't know about the match-arm
// condition, so we propagate narrowings via the `match_arm_narrowing`
// field on `VarResolutionCtx`.  When entering a `match(true)` arm
// body, a new context is created with the narrowed types; callers in
// `resolve_rhs_method_call_inner` and `resolve_rhs_property_access`
// consult `ctx.match_arm_narrowing` when the object is a bare variable.

/// Extract instanceof narrowings from a `match(true)` arm's conditions.
///
/// A condition like `$var instanceof ClassName` maps `"$var"` → the
/// resolved `ClassInfo` for `ClassName`.  The arm runs when any one of its
/// conditions holds, so a variable is narrowed only when every condition
/// names it, to the union of the classes they name: `$p instanceof Cat,
/// $p instanceof Dog` narrows `$p` to `Cat|Dog`, while `$p instanceof Cat,
/// $q instanceof Dog` narrows neither.
fn extract_match_arm_narrowings(
    expr_arm: &MatchExpressionArm<'_>,
    subject_var: Option<&str>,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, Vec<ResolvedType>> {
    let mut narrowed: Option<(String, Vec<ResolvedType>)> = None;
    let names = ctx.written_names();
    for condition in expr_arm.conditions.iter() {
        // `match ($x::class) { Foo::class, Bar::class => … }` — each
        // condition names one class the subject may be, so the arm body
        // sees the union of them.
        let pair = match subject_var {
            Some(var) => {
                crate::type_engine::types::narrowing::class_match_condition_class(condition, &names)
                    .map(|ty| (var.to_owned(), ty))
            }
            None => extract_instanceof_pair(condition, &names),
        };
        // A condition that says nothing this one can read about the
        // variable the others narrow lets the arm run whatever it holds.
        let Some((var_name, class_type)) = pair else {
            return HashMap::new();
        };
        if narrowed.as_ref().is_some_and(|(name, _)| *name != var_name) {
            return HashMap::new();
        }
        let resolved = type_resolution::type_hint_to_classes_typed(
            &class_type,
            &ctx.current_class.fqn(),
            ctx.all_classes,
            ctx.class_loader,
        );
        if resolved.is_empty() {
            return HashMap::new();
        }
        let results = ResolvedType::from_classes_with_hint(resolved, class_type);
        match &mut narrowed {
            Some((_, existing)) => ResolvedType::extend_unique(existing, results),
            None => narrowed = Some((var_name, results)),
        }
    }
    narrowed.into_iter().collect()
}

/// The narrowing a `match (true)` arm's conditions establish for its body.
///
/// This runs the same condition pipeline an `if` body gets, so every rule
/// in it (`!== null`, type guards, assertions, …) reaches a match arm, not
/// just the `instanceof` shape [`extract_match_arm_narrowings`] knows.
///
/// An arm runs when *any* of its conditions matched, so a fact only holds
/// inside the body when every condition proves it: the per-condition maps
/// are intersected by subject and unioned by type.
fn match_true_arm_narrowings(
    expr_arm: &MatchExpressionArm<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, Vec<ResolvedType>> {
    let mut conditions = expr_arm.conditions.iter();
    let Some(first) = conditions.next() else {
        return HashMap::new();
    };
    let mut merged =
        crate::type_engine::variable::forward_walk::condition_narrowing_overrides(first, true, ctx);
    for condition in conditions {
        if merged.is_empty() {
            break;
        }
        let next = crate::type_engine::variable::forward_walk::condition_narrowing_overrides(
            condition, true, ctx,
        );
        merged.retain(|name, types| match next.get(name) {
            Some(other) => {
                ResolvedType::extend_unique(types, other.clone());
                true
            }
            None => false,
        });
    }
    merged
}

/// Extract `($var_name, ClassName)` from `$var instanceof ClassName`.
fn extract_instanceof_pair(
    expr: &Expression<'_>,
    names: &crate::type_engine::resolver::WrittenNames<'_>,
) -> Option<(String, PhpType)> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Binary(bin) = expr
        && bin.operator.is_instanceof()
    {
        // LHS: the variable
        let var_name = match bin.lhs {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return None,
        };
        // RHS: the class name
        let class_type = match bin.rhs {
            Expression::Identifier(ident) => names.class_type(bytes_to_str(ident.value())),
            Expression::Self_(_) => PhpType::named(atom("self")),
            Expression::Static(_) => PhpType::named(atom("static")),
            Expression::Parent(_) => PhpType::named(atom("parent")),
            _ => return None,
        };
        Some((var_name, class_type))
    } else {
        None
    }
}

/// Create a `ResolvedType` from a `PhpType`, looking up class info when the type names a class.
///
/// When the `PhpType` has a `base_name()` that resolves to a known class, returns
/// `ResolvedType::from_both(ty, class)`. Otherwise returns `ResolvedType::from_type_string(ty)`.
fn resolved_type_with_lookup(
    ty: PhpType,
    _current_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> ResolvedType {
    if let Some(base) = ty.base_name() {
        let base = base.strip_prefix('\\').unwrap_or(base);
        // Don't try to look up scalars/pseudo-types
        if !crate::php_type::is_keyword_type(base) {
            let cls = crate::class_lookup::find_or_load_resolved(base, all_classes, class_loader)
                .map(Arc::unwrap_or_clone);
            if let Some(class) = cls {
                return ResolvedType::from_both(ty, class);
            }
        }
    }
    ResolvedType::from_type_string(ty)
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
fn narrowed_subject_from_scope(
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
        None if super::forward_walk::is_diagnostic_scope_active() => {
            if super::forward_walk::is_building_scopes() {
                return None;
            }
            super::forward_walk::lookup_diagnostic_scope(key, expr.span().start.offset)?
        }
        // Only a path read through `->` is worth a walk: the static and
        // offset keys a callee's body reads while its return type is
        // inferred would each walk that body again for every file that
        // calls it, and nothing narrows them often enough to pay for that.
        None if !key.contains("->") => return None,
        None => super::resolution::resolve_variable_types(
            key,
            ctx.current_class,
            ctx.all_classes,
            ctx.content,
            ctx.cursor_offset,
            ctx.class_loader,
            ctx.source_class_loader,
            ctx.source_imports,
            ctx.backend,
            ctx.loaders,
        ),
    };
    (!from_scope.is_empty()).then_some(from_scope)
}

/// The type a check on `call` narrowed it to, given what the method it
/// invokes declares it returns.
///
/// The narrowing is keyed under the call's own text, the same key the
/// subject-expression resolver builds, so the check and every later
/// occurrence of the call agree on what they are talking about.
fn narrowed_call(call: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    let key = crate::type_engine::types::narrowing::expr_to_subject_key(call)?;
    narrowed_subject_from_scope(&key, call, ctx)
}

/// Resolve a right-hand-side expression to zero or more
/// [`ResolvedType`] values.
///
/// This is the single place where an arbitrary PHP expression is
/// resolved to a type.  It handles scalars, array literals,
/// instantiations, calls, property access, match/ternary/null-coalesce,
/// clone, closures, generators, pipe, and bare variables.
///
/// Entries may have `class_info: None` (e.g. scalar literals, array
/// shapes).  Callers that need only class-backed results should
/// filter with [`ResolvedType::into_classes`].
///
/// Used by `check_expression_for_assignment` (for `$var = <expr>`),
/// `check_expression_for_raw_type` (for hover/diagnostics type strings),
/// and recursively by multi-branch constructs (match, ternary, `??`).
///
/// Resolution recurses along the nesting of the expression, so the
/// shapes PHP code really does write long (`$a ?? $b ?? … ?? $z`,
/// ternaries nested in their own `else` branch, stacked `(…)` / `@`
/// wrappers, and fluent method chains) are walked iteratively here
/// instead: they nest one AST level per link, and recursing them would
/// spend a stack frame per link to reach a type the loops below reach
/// for free.
pub(in crate::type_engine) fn resolve_rhs_expression<'b>(
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    if let Some(recorded) = ctx.recorded_expr_type(expr) {
        return recorded.to_vec();
    }
    let expr = peel_type_transparent(expr);
    match expr {
        Expression::Binary(binary) if binary.operator.is_null_coalesce() => {
            resolve_null_coalesce_chain(binary, ctx)
        }
        Expression::Conditional(conditional) => resolve_conditional_chain(conditional, ctx),
        Expression::Call(Call::Method(_) | Call::NullSafeMethod(_)) => {
            resolve_method_chain(expr, ctx)
        }
        Expression::Assignment(assignment) => resolve_assignment_as_value(assignment, ctx),
        _ => resolve_rhs_expression_inner(expr, ctx),
    }
}

/// Resolve an assignment expression used as a value, not just a
/// statement: `$x = ($y = 1)`, `$x = $a ?? ($y = 1) ?? 1`.
///
/// PHP assignment is itself an expression whose value is whatever ends
/// up in the target. Plain `=` yields the RHS's own value; the other
/// operators combine it with the target's current value the same way a
/// statement-level compound assignment does, so an operand position
/// (`??` chain, ternary, match arm) gets the same answer a top-level
/// `$y = expr;` statement would.
///
/// Recording the target itself, so a later read of `$y` sees this
/// write, is the forward walker's job (`process_expr` /
/// `process_assignment_expr`), not this read-only pipeline's.
fn resolve_assignment_as_value<'b>(
    assignment: &'b Assignment<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    use mago_syntax::cst::assignment::AssignmentOperator;
    match assignment.operator {
        AssignmentOperator::Assign(_) => resolve_rhs_expression(assignment.rhs, ctx),
        AssignmentOperator::Coalesce(_) => {
            let lhs_types = resolve_rhs_expression(assignment.lhs, ctx);
            let rhs_types = resolve_rhs_expression(assignment.rhs, ctx);
            let combined = coalesce_assign_value(lhs_types, rhs_types);
            if combined.is_empty() {
                vec![ResolvedType::from_type_string(PhpType::mixed())]
            } else {
                combined
            }
        }
        AssignmentOperator::Concat(_) => vec![ResolvedType::from_type_string(PhpType::string())],
        AssignmentOperator::Modulo(_) => {
            let lhs_types = resolve_rhs_expression(assignment.lhs, ctx);
            let rhs_types = resolve_rhs_expression(assignment.rhs, ctx);
            vec![ResolvedType::from_type_string(infer_modulo_result_type(
                &lhs_types, &rhs_types,
            ))]
        }
        AssignmentOperator::LeftShift(_)
        | AssignmentOperator::RightShift(_)
        | AssignmentOperator::BitwiseAnd(_)
        | AssignmentOperator::BitwiseOr(_)
        | AssignmentOperator::BitwiseXor(_) => vec![ResolvedType::from_type_string(PhpType::int())],
        AssignmentOperator::Addition(_) => {
            let lhs_types = resolve_rhs_expression(assignment.lhs, ctx);
            let rhs_types = resolve_rhs_expression(assignment.rhs, ctx);
            vec![ResolvedType::from_type_string(infer_addition_result_type(
                &lhs_types, &rhs_types,
            ))]
        }
        AssignmentOperator::Subtraction(_)
        | AssignmentOperator::Multiplication(_)
        | AssignmentOperator::Division(_)
        | AssignmentOperator::Exponentiation(_) => {
            let op_kind = match assignment.operator {
                AssignmentOperator::Subtraction(_) => ArithmeticOpKind::Subtraction,
                AssignmentOperator::Multiplication(_) => ArithmeticOpKind::Multiplication,
                AssignmentOperator::Division(_) => ArithmeticOpKind::Division,
                AssignmentOperator::Exponentiation(_) => ArithmeticOpKind::Exponentiation,
                _ => unreachable!("outer match already narrowed to arithmetic operators"),
            };
            let lhs_types = resolve_rhs_expression(assignment.lhs, ctx);
            let rhs_types = resolve_rhs_expression(assignment.rhs, ctx);
            vec![ResolvedType::from_type_string(
                infer_arithmetic_result_type(&lhs_types, &rhs_types, op_kind),
            )]
        }
    }
}

/// What a `??=` leaves behind, given what its target and its fallback
/// resolve to.
///
/// `??=` keeps the target when it is not null and assigns the fallback
/// otherwise, so the value is the target's non-null half unioned with the
/// fallback. The resolved types are combined as they are, rather than
/// joined into one union *type string*, so the `class_info` already
/// attached to each operand survives: a rebuilt string carries none, and
/// a member access on the result would have nothing to resolve against.
///
/// Where both sides name the same type (commonly the target's declared
/// element type, which resolved no class, alongside an argument that did)
/// the class-backed entry speaks for the pair.
///
/// Shared by the `??=` used as a value here and the forward walker's
/// statement-level `??=`.
pub(crate) fn coalesce_assign_value(
    lhs_types: Vec<ResolvedType>,
    rhs_types: Vec<ResolvedType>,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = lhs_types
        .into_iter()
        .filter(|rt| !rt.type_string.is_null())
        .map(|mut rt| {
            if let Some(non_null) = rt.type_string.non_null_type() {
                rt.type_string = non_null;
            }
            rt
        })
        .collect();
    ResolvedType::extend_unique(&mut combined, rhs_types);
    let class_backed: Vec<PhpType> = combined
        .iter()
        .filter(|rt| rt.class_info.is_some())
        .map(|rt| rt.type_string.clone())
        .collect();
    combined.retain(|rt| rt.class_info.is_some() || !class_backed.contains(&rt.type_string));
    combined
}

/// Strip wrappers that cannot change the type of the expression they
/// wrap: parentheses, the error-suppression operator `@`, and `&`.
///
/// `&$expr` used as a value (`$a =& $var`'s own value, an argument to a
/// legacy by-ref call) is whatever `$expr` currently holds; the operator
/// only affects how the *target* is bound, which is the forward walker's
/// concern (`process_expr` / `process_assignment_expr`),
/// not this read-only pipeline's.
fn peel_type_transparent<'b>(mut expr: &'b Expression<'b>) -> &'b Expression<'b> {
    loop {
        match expr {
            Expression::Parenthesized(parenthesized) => expr = parenthesized.expression,
            Expression::UnaryPrefix(unary)
                if unary.operator.is_error_control()
                    || matches!(unary.operator, unary::UnaryPrefixOperator::Reference(_)) =>
            {
                expr = unary.operand
            }
            _ => return expr,
        }
    }
}

/// Resolve a `??` chain, unioning every operand that can still be
/// reached at runtime.
///
/// `??` is right-associative, so `$a ?? $b ?? $c` nests as
/// `$a ?? ($b ?? $c)`; the chain is walked down its right spine rather
/// than recursed into.  Each operand contributes its type with `null`
/// stripped (the next operand covers the null case), an operand that
/// resolves to nothing contributes `mixed` (at runtime it holds *some*
/// value), and an operand that cannot be null at all makes the rest of
/// the chain dead code.
fn resolve_null_coalesce_chain<'b>(
    binary: &'b Binary<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = Vec::new();
    let mut current = binary;
    let base_ctx = ctx;
    let mut operand_ctx: Option<VarResolutionCtx<'_>> = None;
    let mut carried: Option<HashMap<String, Vec<ResolvedType>>> = None;
    loop {
        let ctx = operand_ctx.as_ref().unwrap_or(base_ctx);
        // Syntactically non-nullable operands: `new Foo()`, a non-null
        // literal, an array literal, `clone $x`.
        let non_nullable = match peel_type_transparent(current.lhs) {
            Expression::Literal(Literal::Null(_)) => false,
            Expression::Instantiation(_)
            | Expression::Literal(_)
            | Expression::Array(_)
            | Expression::LegacyArray(_)
            | Expression::Clone(_) => true,
            _ => false,
        };
        let lhs_results = resolve_rhs_expression(current.lhs, ctx);
        if lhs_results.is_empty() && operand_is_null_only(current.lhs, ctx) {
            // A bare variable with no entry in scope has not been
            // assigned on any path reaching this point, so it is
            // undefined here — which PHP evaluates as `null` (with a
            // notice), not "could be anything".  It contributes nothing
            // to the union, the same as a resolved-but-stripped `null`
            // a few lines down.
        } else if lhs_results.is_empty() {
            // A genuinely unresolvable operand. At runtime it could hold
            // any value, so represent it as `mixed` and keep unioning
            // the rest of the chain.
            ResolvedType::extend_unique(
                &mut combined,
                vec![ResolvedType::from_type_string(PhpType::mixed())],
            );
        } else if non_nullable {
            // The remaining operands are unreachable.
            ResolvedType::extend_unique(&mut combined, lhs_results);
            return simplify_branch_results(combined);
        } else {
            ResolvedType::extend_unique(&mut combined, strip_null_alternatives(lhs_results));
        }
        // Always union with the next operand.  Even when the type string
        // looks non-nullable, the user wrote `??` defensively and both
        // branches are valid candidates.
        // An operand after a plain variable runs where that variable was
        // `null`, which the forward walker records at the operand's start
        // (inside `if ($a || $b)`, the `$b` of `$a ?? $b` is truthy).
        if matches!(
            peel_type_transparent(current.lhs),
            Expression::Variable(Variable::Direct(_))
        ) {
            let (own, impossible) =
                crate::type_engine::variable::forward_walk::condition_arm_narrowing(
                    current.lhs,
                    false,
                    ctx,
                );
            let mut next = base_ctx.with_cursor_offset(current.rhs.span().start.offset);
            if !impossible && !own.is_empty() {
                carried
                    .get_or_insert_with(|| base_ctx.match_arm_narrowing.clone())
                    .extend(own);
            }
            if let Some(overrides) = &carried {
                next = next.with_match_arm_narrowing(overrides.clone());
            }
            operand_ctx = Some(next);
        }
        let ctx = operand_ctx.as_ref().unwrap_or(base_ctx);
        match peel_type_transparent(current.rhs) {
            Expression::Binary(next) if next.operator.is_null_coalesce() => current = next,
            last => {
                ResolvedType::extend_unique(&mut combined, resolve_rhs_expression(last, ctx));
                return simplify_branch_results(combined);
            }
        }
    }
}

/// Whether an empty-resolving `??` operand is a bare variable that was
/// never assigned on any path reaching this point, rather than one that
/// was assigned but whose type the resolver failed to work out.
///
/// `resolve_rhs_expression` returns an empty `Vec` for both: the forward
/// walker's `ScopeState` answers an unassigned name and an
/// assigned-but-unresolved one the same way through `scope_var_resolver`
/// (see `ScopeState::snapshot_resolver`).  `scope_contains_resolver` is
/// the one signal that tells them apart, so this only fires when it is
/// available; a caller without a forward-walker scope (e.g. the
/// backward-scan cold path) keeps the existing `mixed`-widening
/// behaviour.
fn operand_is_null_only(expr: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> bool {
    let Expression::Variable(Variable::Direct(dv)) = peel_type_transparent(expr) else {
        return false;
    };
    let Some(contains) = ctx.scope_contains_resolver else {
        return false;
    };
    !contains(bytes_to_str(dv.name))
}

/// Turn a branch that resolved to nothing into `mixed`.
///
/// A union built from branches must never come out narrower than the
/// truth. A branch the resolver could not type still holds *some* value
/// at runtime, so letting it vanish leaves the surviving branches
/// claiming to be the complete set of what the expression can hold, and
/// every consumer reads that as a confident answer: hover names one
/// value, completion offers its members, and a diagnostic that treats the
/// type as exhaustive contradicts the code.
///
/// Branches that produce no value at all (`throw`, `exit`, `die`) are the
/// exception. They never reach the union, so they widen nothing.
fn widen_unresolved_branch(expr: &Expression<'_>, results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    if !results.is_empty() || branch_yields_no_value(expr) {
        return results;
    }
    vec![ResolvedType::from_type_string(PhpType::mixed())]
}

/// Whether a branch expression aborts instead of producing a value.
fn branch_yields_no_value(expr: &Expression<'_>) -> bool {
    matches!(
        peel_type_transparent(expr),
        Expression::Throw(_) | Expression::Construct(Construct::Exit(_) | Construct::Die(_))
    )
}

/// Drop `null` from each resolved alternative of a `??` operand.
///
/// Example: `?Foo ?? Bar` → `Foo|Bar`.  A bare `null` alternative is
/// dropped entirely; anything else (including `mixed`) passes through.
fn strip_null_alternatives(results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    results
        .into_iter()
        .filter_map(|mut resolved| match resolved.type_string.non_null_type() {
            Some(non_null) => {
                resolved.type_string = non_null;
                Some(resolved)
            }
            None if resolved.type_string == PhpType::null() => None,
            None => Some(resolved),
        })
        .collect()
}

/// Keep only what can be truthy in each resolved alternative.
///
/// A short ternary (`$x ?: $default`) yields the condition's own value,
/// but only on the branch where that value was truthy, so `string|false`
/// contributes `string` and nothing more.  An alternative with no truthy
/// part at all (`false`, `null`) is dropped: reaching it is impossible.
fn truthy_alternatives(results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    results
        .into_iter()
        .filter_map(|mut resolved| {
            resolved.type_string = resolved.type_string.truthy_type()?;
            Some(resolved)
        })
        .collect()
}

/// Resolve a ternary, or a chain of ternaries nested in each other's
/// `else` branch (`$a ? 1 : ($b ? 2 : ($c ? 3 : 4))`), to the union of
/// the branches that are reachable.
///
/// Each branch is resolved with the cursor positioned inside it so that
/// instanceof / guard narrowing from the condition applies to variable
/// and property subjects within the branch.  Without this,
/// `$x instanceof Foo ? $x->m() : null` would resolve `$x->m()` against
/// the un-narrowed type, the then branch would fail, and the whole
/// ternary would collapse to the else branch instead of unioning both.
///
/// A condition whose PHP truthiness is statically known (`true`,
/// `false`, `0`, `''`, …) makes one branch unreachable, and the dead
/// branch must not widen the result into a union.  PHPStan prunes the
/// same way.
///
/// A condition that rules out every value one of its subjects holds prunes
/// the same way even though its truthiness is not a constant:
/// `$acc === null ? seed() : $acc->merge()` cannot take its else arm while
/// `$acc` is still exactly `null`, so folding that arm's unresolvable
/// receiver into the union would erase the seed the then arm produces.
fn resolve_conditional_chain<'b>(
    conditional: &'b Conditional<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let mut skipped_impossible = false;
    let results = resolve_conditional_arms(conditional, ctx, true, &mut skipped_impossible);
    // Pruning must never be the reason the whole ternary has no type: when
    // every arm that was left looked impossible, the narrowing that said so
    // was the less trustworthy of the two answers.
    if results.is_empty() && skipped_impossible {
        return resolve_conditional_arms(conditional, ctx, false, &mut false);
    }
    results
}

fn resolve_conditional_arms<'b>(
    conditional: &'b Conditional<'b>,
    ctx: &VarResolutionCtx<'_>,
    prune_impossible: bool,
    skipped_impossible: &mut bool,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = Vec::new();
    let mut current = conditional;
    // Narrowing carried down the else spine: reaching link N means every
    // earlier condition was false, so their inverse specifications all hold.
    let mut carried = ctx.match_arm_narrowing.clone();
    loop {
        // A short ternary (`$a ?: $b`) reuses the condition as its then
        // branch.
        let is_short = current.then.is_none();
        let then_expr = current.then.unwrap_or(current.condition);
        let truthiness = static_condition_truthiness(current.condition);
        if truthiness != Some(false) {
            let then_ctx = ctx.with_cursor_offset(then_expr.span().start.offset);
            // The then arm only runs when the condition held, so it sees the
            // condition's positive narrowing — the same specification an
            // `if` body gets.
            let (then_ctx, impossible) =
                with_arm_narrowing(&then_ctx, &carried, current.condition, true);
            if impossible && prune_impossible {
                *skipped_impossible = true;
            } else {
                let then_results = resolve_rhs_expression(then_expr, &then_ctx);
                let then_results = widen_unresolved_branch(then_expr, then_results);
                // The short form yields the condition's value, and reaching it
                // means the condition was truthy, so its falsy members are not
                // part of the result.
                let then_results = if is_short {
                    truthy_alternatives(then_results)
                } else {
                    then_results
                };
                ResolvedType::extend_unique(&mut combined, then_results);
            }
        }
        if truthiness == Some(true) {
            return simplify_branch_results(combined);
        }
        match peel_type_transparent(current.r#else) {
            Expression::Conditional(next) => {
                merge_arm_narrowing(&mut carried, current.condition, false, ctx);
                current = next;
            }
            _ => {
                let else_ctx = ctx.with_cursor_offset(current.r#else.span().start.offset);
                // Reaching the else arm proves the condition was false, so it
                // sees the inverse narrowing.
                let (else_ctx, impossible) =
                    with_arm_narrowing(&else_ctx, &carried, current.condition, false);
                if impossible && prune_impossible {
                    *skipped_impossible = true;
                } else {
                    let else_results = resolve_rhs_expression(current.r#else, &else_ctx);
                    ResolvedType::extend_unique(
                        &mut combined,
                        widen_unresolved_branch(current.r#else, else_results),
                    );
                }
                return simplify_branch_results(combined);
            }
        }
    }
}

/// Derive a context whose variable lookups see `condition`'s narrowing for
/// the given polarity, layered on top of `carried`, and say whether the
/// polarity is possible at all.
///
/// The condition's own specification wins over `carried`: it is evaluated
/// against the same declared types and is the more specific of the two.
fn with_arm_narrowing<'a>(
    ctx: &VarResolutionCtx<'a>,
    carried: &HashMap<String, Vec<ResolvedType>>,
    condition: &Expression<'_>,
    truthy: bool,
) -> (VarResolutionCtx<'a>, bool) {
    let mut overrides = carried.clone();
    let (own, impossible) =
        crate::type_engine::variable::forward_walk::condition_arm_narrowing(condition, truthy, ctx);
    overrides.extend(own);
    (ctx.with_match_arm_narrowing(overrides), impossible)
}

/// Fold `condition`'s narrowing for one polarity into an override map.
fn merge_arm_narrowing(
    overrides: &mut HashMap<String, Vec<ResolvedType>>,
    condition: &Expression<'_>,
    truthy: bool,
    ctx: &VarResolutionCtx<'_>,
) {
    for (name, types) in crate::type_engine::variable::forward_walk::condition_narrowing_overrides(
        condition, truthy, ctx,
    ) {
        overrides.insert(name, types);
    }
}

/// One link of a fluent chain: `->method(args)` applied to `object`.
struct ChainLink<'b> {
    /// The call expression the link was peeled from, which is what a
    /// check written on the call is keyed under.
    call: &'b Expression<'b>,
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    nullsafe: bool,
}

/// Resolve a method call, walking its receiver spine iteratively.
///
/// `$x->a()->b()->c()` nests one method call per link, and each link's
/// receiver is the link before it — a chain, not a tree.  Resolving the
/// outermost call and recursing into its receiver therefore costs a stack
/// frame per link, so a long enough chain overflows the stack whatever
/// size it is given.  Hand-written code stays short, but generated query
/// builders and generated API clients do not.
///
/// So the spine is peeled into a `Vec` and folded outward from the base
/// instead: only the innermost link resolves an object expression (which
/// is where `$this`, a bare variable, or `(new Foo)` is read), and every
/// link after it is handed the previous link's result as its receiver.
///
/// A chain that mixes in property reads (`$x->a()->prop->b()`) breaks the
/// spine at the property, which resolves through the normal recursive
/// path; only the method links either side of it are flattened.
fn resolve_method_chain<'b>(
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let mut links: Vec<ChainLink<'b>> = Vec::new();
    let mut current = expr;
    loop {
        let peeled = peel_type_transparent(current);
        let link = match peeled {
            Expression::Call(Call::Method(call)) => ChainLink {
                call: peeled,
                object: call.object,
                method: &call.method,
                argument_list: &call.argument_list,
                nullsafe: false,
            },
            Expression::Call(Call::NullSafeMethod(call)) => ChainLink {
                call: peeled,
                object: call.object,
                method: &call.method,
                argument_list: &call.argument_list,
                nullsafe: true,
            },
            // The base of the spine.  It is left to the innermost link,
            // which knows how to read it as a receiver.
            _ => break,
        };
        current = link.object;
        links.push(link);
    }

    let mut receiver: Option<MethodReceiver> = None;
    for link in links.iter().rev() {
        let (owners, receiver_resolved) =
            receiver.unwrap_or_else(|| method_calls::resolve_method_receiver(link.object, ctx));
        let nullability = ReceiverNullability::of(&receiver_resolved);
        let mut resolved = resolve_method_call_on_receiver(
            link.object,
            link.method,
            link.argument_list,
            Some((owners, receiver_resolved)),
            ctx,
        );
        calls::finish_return_constant_operands(&mut resolved, ctx);
        apply_nullsafe_short_circuit(&mut resolved, nullability, link.nullsafe, link.object);
        // A check written on the call itself (`if ($h->get() instanceof
        // Foo)`, `if ($this->option('from') !== null)`) is keyed under the
        // call's own text, so a later occurrence of that text reads the
        // narrowed type instead of the method's declared return type.
        if let Some(narrowed) = narrowed_call(link.call, ctx) {
            resolved = narrowed;
        }
        receiver = Some((ResolvedType::into_arced_classes(resolved.clone()), resolved));
    }
    // `links` always holds at least the call this was entered on.
    receiver.map(|(_, resolved)| resolved).unwrap_or_default()
}

fn resolve_rhs_expression_inner<'b>(
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    match expr {
        // ── Scalar literals ─────────────────────────────────────────
        Expression::Literal(Literal::Integer(integer)) => {
            let ty = integer
                .value
                .map(|value| PhpType::literal_int(value.to_string()))
                .unwrap_or_else(PhpType::int);
            vec![ResolvedType::from_type_string(ty)]
        }
        Expression::Literal(Literal::Float(float)) => {
            vec![ResolvedType::from_type_string(PhpType::literal_float(
                bytes_to_str(float.raw).to_string(),
            ))]
        }
        Expression::Literal(Literal::String(string)) => {
            vec![ResolvedType::from_type_string(PhpType::literal_string_raw(
                bytes_to_str(string.raw).to_string(),
            ))]
        }
        // A written `true` / `false` keeps its literal type the way every
        // other literal does, so a variable seeded with `false` can have
        // that half subtracted by a later truthiness check instead of
        // carrying an unfalsifiable `bool`.
        Expression::Literal(Literal::True(_)) => {
            vec![ResolvedType::from_type_string(PhpType::true_())]
        }
        Expression::Literal(Literal::False(_)) => {
            vec![ResolvedType::from_type_string(PhpType::false_())]
        }
        Expression::Literal(Literal::Null(_)) => {
            vec![ResolvedType::from_type_string(PhpType::null())]
        }
        // ── Array literals ──────────────────────────────────────────
        Expression::Array(arr) => {
            let pt =
                super::raw_type_inference::infer_array_literal_raw_type(arr.elements.iter(), ctx)
                    .unwrap_or_else(PhpType::array);
            vec![ResolvedType::from_type_string(pt)]
        }
        Expression::LegacyArray(arr) => {
            let pt =
                super::raw_type_inference::infer_array_literal_raw_type(arr.elements.iter(), ctx)
                    .unwrap_or_else(PhpType::array);
            vec![ResolvedType::from_type_string(pt)]
        }
        Expression::Instantiation(inst) => resolve_rhs_instantiation(inst, ctx),
        // ── Anonymous class: `new class extends Foo { … }` ──────────
        // The parser stores these in `all_classes` with a synthetic
        // name `__anonymous@<offset>`.  Look it up by matching the
        // left-brace offset so the variable inherits the full
        // ClassInfo (parent class, traits, methods, etc.).
        Expression::AnonymousClass(anon) => {
            let start = anon.left_brace.start.offset;
            let name = format!("__anonymous@{}", start);
            if let Some(cls) = ctx.all_classes.iter().find(|c| c.name == name) {
                return ResolvedType::from_classes(vec![Arc::clone(cls)]);
            }
            vec![]
        }
        Expression::ArrayAccess(array_access) => {
            // Check if the scope has a narrowed type for this array
            // access (e.g. `$a["test"]` narrowed through null checks, or
            // `$config['class']` narrowed to `class-string<Foo>` by an
            // `is_a(..., true)` guard).  The completion/hover paths carry
            // a `scope_var_resolver`; the diagnostic path instead reads
            // the forward walker's snapshot cache, so both are consulted.
            if let Some(key) = crate::type_engine::types::narrowing::expr_to_subject_key(expr)
                && key.contains('[')
                && let Some(from_scope) = narrowed_subject_from_scope(&key, expr, ctx)
            {
                return from_scope;
            }
            resolve_rhs_array_access(array_access, expr, ctx)
        }
        // A function or static call checked earlier in the scope
        // (`if (mb_strpos($s, $m) !== false)`) reads the narrowed entry
        // the check left under the call's own text.  Method calls get the
        // same treatment one level up, in `resolve_method_chain`, where
        // the chain spine is already peeled.
        Expression::Call(call @ (Call::Function(_) | Call::StaticMethod(_))) => {
            narrowed_call(expr, ctx).unwrap_or_else(|| resolve_rhs_call(call, expr, ctx))
        }
        Expression::Call(call) => resolve_rhs_call(call, expr, ctx),
        Expression::Access(access) => {
            // A ternary or match arm narrows the whole path, not just the
            // variable it is rooted at: `$a->alt ? $a->alt : $a->title`
            // proves `$a->alt` truthy for its own then-arm, and that
            // proof is keyed under the path.  Checked before the scope
            // lookup because the arm describes this branch specifically,
            // where the scope entry describes the statement as a whole.
            if !ctx.match_arm_narrowing.is_empty()
                && let Some(key) = crate::type_engine::types::narrowing::expr_to_subject_key(expr)
                && let Some(narrowed) = ctx.match_arm_narrowing.get(&key)
            {
                return narrowed.clone();
            }
            // Check if the scope has a narrowed type for this property
            // access (e.g. `$a->foo` narrowed through if/elseif
            // conditions, or assigned inside a guarded `if`).  A static
            // property (`self::$repo`) is read the same way: the walker
            // records writes and checks under its own key, and reading
            // straight from the declaration is what loses them.
            if let Some(key) = crate::type_engine::types::narrowing::expr_to_subject_key(expr)
                && (key.contains("->") || key.contains("::$"))
                && let Some(from_scope) = narrowed_subject_from_scope(&key, expr, ctx)
            {
                return from_scope;
            }
            resolve_rhs_property_access(access, ctx)
        }
        // Unary signs are separate AST nodes rather than part of numeric
        // literals. Resolve the operand first so parenthesized expressions,
        // exact variables, and compound literal branches keep their values.
        Expression::UnaryPrefix(unary)
            if matches!(
                unary.operator,
                mago_syntax::cst::unary::UnaryPrefixOperator::Negation(_)
                    | mago_syntax::cst::unary::UnaryPrefixOperator::Plus(_)
            ) =>
        {
            use mago_syntax::cst::unary::UnaryPrefixOperator;

            let negated = matches!(unary.operator, UnaryPrefixOperator::Negation(_));
            let operand = resolve_rhs_expression(unary.operand, ctx);
            let signed = (!operand.is_empty())
                .then(|| ResolvedType::types_joined(&operand))
                .and_then(|ty| apply_numeric_sign(&ty, negated));
            vec![ResolvedType::from_type_string(signed.unwrap_or_else(
                || PhpType::union(vec![PhpType::int(), PhpType::float()]),
            ))]
        }
        // `++$x` / `--$x` evaluate to the value *after* the step, so a
        // numeric operand keeps its own domain (`int` stays `int` rather
        // than widening to `array-key` when used as an array write key).
        Expression::UnaryPrefix(unary) if unary.operator.is_increment_or_decrement() => {
            match stepped_numeric_type(&resolve_rhs_expression(unary.operand, ctx)) {
                Some(ty) => vec![ResolvedType::from_type_string(ty)],
                None => vec![],
            }
        }
        // `$x++` / `$x--` evaluate to the value *before* the step, which is
        // exactly the operand's type.
        Expression::UnaryPostfix(unary) => resolve_rhs_expression(unary.operand, ctx),
        // Type casts (`(int) $x`), `!`, and `~`.  The result depends on the
        // operator, not on how the expression is reached, so a cast in a
        // ternary branch resolves the same as one assigned directly.
        Expression::UnaryPrefix(unary) => {
            match unary_prefix_result_type(&unary.operator, unary.operand, || {
                resolve_rhs_expression(unary.operand, ctx)
            }) {
                Some(ty) => vec![ResolvedType::from_type_string(ty)],
                None => vec![],
            }
        }
        Expression::Match(match_expr) => {
            // Two subject shapes carry narrowing information: `match (true)`
            // with `instanceof` conditions, and `match ($x::class)` with
            // `Foo::class` conditions.
            let subject_var = crate::type_engine::types::narrowing::match_class_subject_var(
                match_expr.expression,
            );
            let is_true_subject = match_expr.expression.is_true();
            let narrows = is_true_subject || subject_var.is_some();
            let mut combined = Vec::new();
            // Reaching an arm means every arm above it was tested and
            // failed, so the inverse of their conditions holds in its body
            // — the proof a ternary chain carries down its `else` spine,
            // and what makes `default => $x` see the `$x === null` arm.
            let mut carried: HashMap<String, Vec<ResolvedType>> = HashMap::new();
            // `default` runs only once every other arm failed, wherever it is
            // written, so it is resolved last.
            let (default_arms, expression_arms): (Vec<_>, Vec<_>) = match_expr
                .arms
                .iter()
                .partition(|arm| matches!(arm, MatchArm::Default(_)));
            for arm in expression_arms.into_iter().chain(default_arms) {
                // Narrowed variable types, so that the arm expression
                // resolves against the narrowed class.
                let mut overrides = carried.clone();
                if narrows && let MatchArm::Expression(expr_arm) = arm {
                    // The arm's own conditions describe its body more
                    // precisely than the carried inverses, so they win.
                    overrides.extend(extract_match_arm_narrowings(expr_arm, subject_var, ctx));
                    if subject_var.is_none() {
                        // The shared pipeline needs the forward walker's
                        // scope to seed the subjects; where there is none
                        // the `instanceof` extractor above is all we get.
                        overrides.extend(match_true_arm_narrowings(expr_arm, ctx));
                    }
                }
                // The arm is resolved with the cursor inside it, as a
                // ternary's branches are: a lookup that has no scope to read
                // walks to the cursor, and the walk applies the narrowing
                // the arm runs under there.
                let arm_expr = arm.expression();
                let mut arm_ctx = ctx.with_cursor_offset(arm_expr.span().start.offset);
                if !overrides.is_empty() {
                    arm_ctx.match_arm_narrowing = overrides;
                }
                let arm_results = resolve_rhs_expression(arm_expr, &arm_ctx);
                ResolvedType::extend_unique(
                    &mut combined,
                    widen_unresolved_branch(arm_expr, arm_results),
                );
                if is_true_subject && let MatchArm::Expression(expr_arm) = arm {
                    // Every condition of this arm was not `true` for the
                    // arms below it, so all of their inverses hold together,
                    // on top of what the arms above already proved.
                    for condition in expr_arm.conditions.iter() {
                        if !super::forward_walk::match_true_condition_is_boolean(condition, || {
                            resolve_rhs_expression(condition, ctx)
                        }) {
                            continue;
                        }
                        let carried_ctx = (!carried.is_empty())
                            .then(|| ctx.with_match_arm_narrowing(carried.clone()));
                        carried.extend(super::forward_walk::condition_narrowing_overrides(
                            condition,
                            false,
                            carried_ctx.as_ref().unwrap_or(ctx),
                        ));
                    }
                }
            }
            simplify_branch_results(combined)
        }
        Expression::Clone(clone_expr) => resolve_rhs_clone(clone_expr, ctx),
        // ── Pipe operator (PHP 8.5): `$expr |> callable(...)` ──
        // The result type is the return type of the callable.
        // The callable is typically a first-class callable reference
        // (PartialApplication) such as `trim(...)` or `createDate(...)`.
        Expression::Pipe(pipe) => resolve_rhs_pipe(pipe, ctx),
        Expression::PartialApplication(_)
        | Expression::Closure(_)
        | Expression::ArrowFunction(_) => {
            // Closures produce a `Closure` instance at runtime, but when we
            // can infer their body return type (explicit `: T`, generator
            // yields, or arrow-body expression), preserve it in the
            // `TypeKind::Callable` so callers like template binding can use
            // it through `$closure` variables.
            let closure_ty = infer_closure_literal_type(expr, ctx);
            // Always resolve against the plain Closure class so that
            // methods like bindTo() are available for completion, even
            // when the inferred type is a typed Callable (Closure(): T).
            let lookup_ty = PhpType::closure();
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &lookup_ty,
                &ctx.current_class.fqn(),
                ctx.all_classes,
                ctx.class_loader,
            );
            if classes.is_empty() {
                vec![ResolvedType::from_type_string(closure_ty)]
            } else {
                ResolvedType::from_classes_with_hint(classes, closure_ty)
            }
        }
        // ── Generator yield-assignment: `$var = yield $expr` ──
        // The value of a yield expression is the TSend type from
        // the enclosing function's `@return Generator<K, V, TSend, R>`.
        Expression::Yield(_) => {
            if let Some(ref ret_type) = ctx.enclosing_return_type
                && let Some(send_php_type) = ret_type.generator_send_type(true)
            {
                return ResolvedType::from_classes_with_hint(
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        send_php_type,
                        &ctx.current_class.fqn(),
                        ctx.all_classes,
                        ctx.class_loader,
                    ),
                    send_php_type.clone(),
                );
            }
            vec![]
        }
        // ── Bare variable: `$a = $b` ────────────────────────────────
        // Resolve the RHS variable's type by walking assignments before
        // this point.  The caller (`check_expression_for_assignment`)
        // already set `ctx.cursor_offset` to the assignment's start
        // offset, so the recursive resolution only considers
        // assignments *before* the current one, preventing cycles.
        Expression::Variable(Variable::Direct(dv)) => {
            let rhs_var = bytes_to_str(dv.name).to_string();
            // A match arm may have narrowed the variable (`match ($x::class)
            // { Foo::class => $x }`), in which case the arm's type wins over
            // the declared one.
            if let Some(narrowed) = ctx.match_arm_narrowing.get(&rhs_var) {
                return narrowed.clone();
            }
            // Guard: never recurse into the same variable (self-assignment).
            if rhs_var == ctx.var_name {
                return vec![];
            }
            resolve_var_types(&rhs_var, ctx, ctx.cursor_offset)
        }
        // ── Concatenation: `"prefix" . $var` → string ───────────────
        // Two operands that each hold one known scalar concatenate to the
        // literal PHP would build (`'1' . 'a'` is `'1a'`).
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            let folded = (scalar_fold::is_cheap_scalar_operand(binary.lhs)
                && scalar_fold::is_cheap_scalar_operand(binary.rhs))
            .then(|| {
                let lhs = scalar_fold::single_scalar(&resolve_rhs_expression(binary.lhs, ctx))?;
                let rhs = scalar_fold::single_scalar(&resolve_rhs_expression(binary.rhs, ctx))?;
                scalar_fold::fold_concat(&lhs, &rhs)
            })
            .flatten();
            vec![ResolvedType::from_type_string(
                folded.unwrap_or_else(PhpType::string),
            )]
        }
        // ── Interpolated strings: `"$a-$b"`, heredocs ───────────────
        // Command substitution (backticks) is not a plain string and is
        // left unresolved.
        Expression::CompositeString(string)
            if !matches!(
                string,
                mago_syntax::cst::string::CompositeString::ShellExecute(_)
            ) =>
        {
            let folded =
                scalar_fold::fold_interpolated(string, |part| resolve_rhs_expression(part, ctx));
            vec![ResolvedType::from_type_string(
                folded.unwrap_or_else(PhpType::string),
            )]
        }
        // ── Magic constants: `__LINE__`, `__FILE__`, `__CLASS__`, … ─
        Expression::MagicConstant(magic) => {
            vec![ResolvedType::from_type_string(magic_constant_type(
                magic, ctx,
            ))]
        }
        // ── Global constant access: `PHP_EOL`, `SORT_ASC`, etc. ────
        Expression::ConstantAccess(ca) => {
            let name = bytes_to_str(ca.name.value()).to_string();
            let name_clean = strip_fqn_prefix(&name);
            // `true`, `false`, `null` are parsed as ConstantAccess by
            // some AST variants — handle them the same as literals.
            match name_clean.to_lowercase().as_str() {
                "true" | "false" => {
                    return vec![ResolvedType::from_type_string(PhpType::bool())];
                }
                "null" => {
                    return vec![ResolvedType::from_type_string(PhpType::null())];
                }
                _ => {}
            }
            if let Some(ts) = global_constant_type(&name, ca.name.span().start.offset, ctx) {
                return vec![ResolvedType::from_type_string(ts)];
            }
            crate::hover::constants::unversioned_php_version_constant_type(name_clean)
                .map(|ts| vec![ResolvedType::from_type_string(ts)])
                .unwrap_or_default()
        }
        Expression::Construct(Construct::Isset(_) | Construct::Empty(_)) => {
            vec![ResolvedType::from_type_string(PhpType::bool())]
        }
        // ── Arithmetic and other binary operators ───────────────────
        // `??` and method-chain binaries are peeled off in
        // `resolve_rhs_expression` before this function is reached, and
        // concatenation is handled above; everything else (arithmetic,
        // comparison, bitwise, spaceship) is answered by operator kind.
        Expression::Binary(binary) => resolve_binary_result_type(binary, ctx).unwrap_or_default(),
        // ── Catch-all: unrecognised expression types ────────────────
        // Return an empty vec — callers that need a type string for
        // expressions not handled above should use the raw-type
        // inference pipeline.
        _ => vec![],
    }
}

/// The type the global constant `name` holds, read from its initializer.
///
/// `offset` is where `name` is written, or `0` when it was not written in
/// the file at all. A name the initializer itself uses is looked up through
/// the same constant loader, so an unqualified `ONE` in `const TWO = ONE * 2;`
/// finds the reading namespace's `ONE` the way the reference to `TWO` found
/// that namespace's `TWO`. The text-only resolver the fold falls back to has
/// no namespace to try.
fn global_constant_type(name: &str, offset: u32, ctx: &VarResolutionCtx<'_>) -> Option<PhpType> {
    let value = ctx.lookup_constant(name, offset)??;
    infer_type_from_constant_value(&value).or_else(|| {
        let rctx = ctx.as_resolution_ctx();
        let resolve = |text: &str| {
            let text = text.trim();
            let is_constant_name = text
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'\\')
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'\\');
            is_constant_name
                .then(|| global_constant_type(text, 0, ctx))
                .flatten()
                .or_else(|| crate::Backend::resolve_arg_text_to_type(text, &rctx))
        };
        crate::type_engine::types::const_fold::folded_constant_type(
            strip_fqn_prefix(name),
            &value,
            &resolve,
        )
    })
}

/// Resolve a pipe expression `$input |> callable(...)` to the callable's
/// return type.
///
/// The pipe operator passes `$input` as the first argument to `callable`
/// and returns its result.  Chains like `$a |> f(...) |> g(...)` are
/// nested: the outer pipe's input is the inner pipe expression.
///
/// Currently handles function-level callables (e.g. `createDate(...)`).
/// Method and static method callables are not yet supported.
fn resolve_rhs_pipe(pipe: &Pipe<'_>, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    // The callable determines the result type.
    // For `PartialApplication::Function`, extract the function name
    // and look up its return type.
    match pipe.callable {
        Expression::PartialApplication(PartialApplication::Function(fpa)) => {
            let func_name = match fpa.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return vec![],
            };
            let func_name_offset = fpa.function.span().start.offset;
            if let Some(fl) = ctx.function_loader()
                && let Some(func_info) = fl(&func_name, func_name_offset)
                && let Some(ref ret) = func_info.return_type
            {
                return ResolvedType::from_classes_with_hint(
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        ret,
                        &ctx.current_class.fqn(),
                        ctx.all_classes,
                        ctx.class_loader,
                    ),
                    ret.clone(),
                );
            }
            vec![]
        }
        // Method callable: `$input |> $obj->method(...)`
        // Static callable: `$input |> Class::method(...)`
        // Not yet supported — fall back to empty.
        _ => vec![],
    }
}

/// Resolve `clone $expr` — preserves the cloned expression's type.
///
/// First tries resolving the inner expression structurally (handles
/// `clone new Foo()`, `clone $this->getConfig()`, ternary, etc.).
/// If that yields nothing, falls back to text-based resolution by
/// extracting the source text of the cloned expression and resolving
/// it as a subject string via `resolve_target_classes`.
fn resolve_rhs_clone(clone_expr: &Clone<'_>, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    let structural = resolve_rhs_expression(clone_expr.object, ctx);
    if !structural.is_empty() {
        return structural;
    }
    // Fallback: extract source text of the cloned expression
    // and resolve it as a subject.  This handles cases like
    // `clone $original` where `$original`'s type was set by a
    // prior assignment or parameter type hint.
    let obj_span = clone_expr.object.span();
    let start = obj_span.start.offset as usize;
    let end = obj_span.end.offset as usize;
    if end <= ctx.content.len() {
        let obj_text = ctx.content[start..end].trim();
        if !obj_text.is_empty() {
            let rctx = ctx.as_resolution_ctx();
            return crate::type_engine::resolver::resolve_target_classes(
                obj_text,
                crate::types::AccessKind::Arrow,
                &rctx,
            );
        }
    }
    vec![]
}

/// Extract the return type hint from a closure or arrow function expression.
///
/// Returns the type-hint string when the expression is a `Closure` or
/// `ArrowFunction` with an explicit return type annotation, e.g.
/// `fn (): Foo => …` yields `Foo`, qualified against the file's imports.
/// Returns `None` otherwise.
fn extract_closure_or_arrow_return_type(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let hint = match expr {
        Expression::ArrowFunction(arrow) => arrow.return_type_hint.as_ref()?,
        Expression::Closure(closure) => closure.return_type_hint.as_ref()?,
        _ => return None,
    };
    Some(
        ctx.written_names()
            .resolve_hint(&extract_hint_type(&hint.hint)),
    )
}

/// Infer template parameter substitutions from a `@psalm-if-this-is` pattern
/// by matching it against the receiver's concrete type.
///
/// For example, given:
/// - `pattern`: `ArrayList<TOption|TEither>`
/// - `receiver`: `ArrayList<Either<Exception, int>|Option<int>>`
/// - Method templates: `A`, `B`, `TOption of Option<A>`, `TEither of Either<mixed, B>`
///
/// This matches `TOption → Option<int>`, `TEither → Either<Exception, int>`,
/// then extracts `A = int` from `Option<A>` vs `Option<int>`, and
/// `B = int` from `Either<mixed, B>` vs `Either<Exception, int>`.
pub(crate) fn infer_if_this_is_subs(
    pattern: &PhpType,
    receiver: &PhpType,
    template_params: &[Atom],
    template_bounds: &AtomMap<PhpType>,
) -> HashMap<String, PhpType> {
    let mut subs: HashMap<String, PhpType> = HashMap::new();

    // Step 1: Match the top-level structure (e.g. Generic vs Generic)
    // and collect direct template bindings.
    match_type_pattern(
        pattern,
        receiver,
        template_params,
        template_bounds,
        &mut subs,
    );

    // Step 2: For each matched template that has a bound with nested
    // templates, match the bound against the concrete value to extract
    // the nested template parameters.
    let direct_subs = subs.clone();
    for (tpl_name, concrete_type) in &direct_subs {
        let tpl_atom = crate::atom::atom(tpl_name);
        if let Some(bound) = template_bounds.get(&tpl_atom) {
            match_type_pattern(
                bound,
                concrete_type,
                template_params,
                template_bounds,
                &mut subs,
            );
        }
    }

    subs
}

/// Recursively match a type pattern against a concrete type, collecting
/// template parameter bindings into `subs`.
fn match_type_pattern(
    pattern: &PhpType,
    concrete: &PhpType,
    template_params: &[Atom],
    template_bounds: &AtomMap<PhpType>,
    subs: &mut HashMap<String, PhpType>,
) {
    match (pattern.kind(), concrete.kind()) {
        // A named type that is a template parameter — bind it.
        (TypeKind::Named(name), _)
            if template_params.iter().any(|t| t.as_str() == name.as_str()) =>
        {
            subs.entry(name.to_string())
                .or_insert_with(|| concrete.clone());
        }
        // Generic types with matching base names — recurse into args.
        (TypeKind::Generic(p), TypeKind::Generic(c))
            if p.name == c.name && p.args.len() == c.args.len() =>
        {
            for (p_arg, c_arg) in p.args.iter().zip(c.args.iter()) {
                match_type_pattern(p_arg, c_arg, template_params, template_bounds, subs);
            }
        }
        // Union types — match pattern members against concrete members
        // by trying to pair each template pattern member with a concrete
        // member whose base name matches the template's bound.
        (TypeKind::Union(p_members), TypeKind::Union(c_members)) => {
            for p_m in p_members {
                if let TypeKind::Named(name) = p_m.kind() {
                    if template_params.iter().any(|t| t.as_str() == name.as_str()) {
                        // This pattern member is a template param in a union.
                        // Find the concrete union member whose base name
                        // matches this template's bound base name.
                        let tpl_atom = crate::atom::atom(name);
                        if let Some(bound) = template_bounds.get(&tpl_atom) {
                            let bound_base = bound.base_name().unwrap_or_default();
                            for c_m in c_members {
                                let c_base = c_m.base_name().unwrap_or_default();
                                if c_base == bound_base {
                                    subs.entry(name.to_string()).or_insert_with(|| c_m.clone());
                                    break;
                                }
                            }
                        } else {
                            // No bound — take the first concrete member.
                            if let Some(c_m) = c_members.first() {
                                subs.entry(name.to_string()).or_insert_with(|| c_m.clone());
                            }
                        }
                    }
                } else {
                    // Non-template pattern member — recurse.
                    for c_m in c_members {
                        if p_m.base_name() == c_m.base_name() {
                            match_type_pattern(p_m, c_m, template_params, template_bounds, subs);
                            break;
                        }
                    }
                }
            }
        }
        // Nullable patterns.
        (TypeKind::Nullable(p_inner), TypeKind::Nullable(c_inner)) => {
            match_type_pattern(p_inner, c_inner, template_params, template_bounds, subs);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_sign_distinguishes_number_pseudo_type_from_number_class() {
        assert_eq!(
            apply_numeric_sign(&PhpType::named(atom("number")), true),
            Some(PhpType::union(vec![PhpType::int(), PhpType::float()]))
        );
        assert_eq!(
            apply_numeric_sign(&PhpType::named(atom("Number")), true),
            None
        );
    }

    #[test]
    fn branch_simplification_keeps_nested_mixed_alternatives() {
        let nested_mixed = PhpType::union(vec![
            PhpType::mixed(),
            PhpType::named(atom("CompletionFallback")),
        ]);
        let results = vec![
            ResolvedType::from_type_string(nested_mixed.clone()),
            ResolvedType::from_type_string(PhpType::literal_string_raw("'tag'")),
        ];

        let simplified = simplify_branch_results(results);
        assert_eq!(simplified.len(), 2);
        assert_eq!(simplified[0].type_string, nested_mixed);
        assert_eq!(
            simplified[1].type_string,
            PhpType::literal_string_raw("'tag'")
        );
    }

    #[test]
    fn branch_simplification_preserves_class_metadata_while_absorbing_scalar_literal() {
        let class = crate::test_fixtures::make_class("Example");
        let results = vec![
            ResolvedType::from_class(class),
            ResolvedType::from_type_string(PhpType::literal_string_raw("'fixed'")),
            ResolvedType::from_type_string(PhpType::string()),
        ];

        let simplified = simplify_branch_results(results);
        assert_eq!(simplified.len(), 2);
        assert!(simplified.iter().any(|result| result.class_info.is_some()));
        assert!(simplified.iter().any(|result| {
            result.class_info.is_none() && result.type_string == PhpType::string()
        }));
    }
}
