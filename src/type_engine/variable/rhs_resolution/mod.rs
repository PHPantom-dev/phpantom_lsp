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
//!   one owner class's method for both instance and static calls, and
//!   [`method_chain`] folding a fluent chain outward from its base.
//! - [`laravel_calls`]: Laravel helpers and accessors whose arguments
//!   decide their return type.
//! - [`property_access`]: `$this->prop` / `$obj->prop` resolution and
//!   the `find_*_this_property_assignment*` scanners.
//! - [`nullsafe`]: what a `?->` on a receiver that can be `null` does to
//!   the rest of the chain.
//! - [`scope_reads`]: variable reads and narrowed member paths, answered
//!   from the forward walker's scope.
//! - [`unary_prefix`]: casts, `!`, `~`, the signs, and `++`/`--`.
//! - [`magic_constants`]: `__LINE__`, `__CLASS__`, `__METHOD__`, and the
//!   other magic constants; [`constant_access`]: global constants.
//! - [`literal_text`]: types read from a constant initializer's or an
//!   array literal's source text.
//! - [`branches`]: ternaries, `??` chains, and `match`, each branch
//!   resolved under its condition's narrowing.
//! - [`assignment_value`]: the value an assignment used as an expression
//!   evaluates to, including `??=` and the compound operators.

use std::sync::Arc;

use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

mod arithmetic;
mod array_access;
mod assignment_value;
mod branches;
mod calls;
mod constant_access;
mod function_calls;
mod instantiation;
mod laravel_calls;
mod literal_text;
mod magic_constants;
mod method_calls;
mod method_chain;
mod nullsafe;
mod owner_method_call;
mod property_access;
mod scalar_fold;
mod scope_reads;
mod static_calls;
mod unary_prefix;

pub(crate) use scalar_fold::{fold_concat_types, is_literal_string_type};

use arithmetic::resolve_binary_result_type;
use array_access::resolve_rhs_array_access;
use assignment_value::resolve_assignment_as_value;
use branches::{resolve_conditional_chain, resolve_match_expression, resolve_null_coalesce_chain};
use calls::resolve_rhs_call;
use constant_access::resolve_rhs_constant_access;
use instantiation::resolve_rhs_instantiation;
use magic_constants::magic_constant_type;
use method_chain::resolve_method_chain;
use property_access::resolve_rhs_property_access;
use scope_reads::{narrowed_call, narrowed_subject_from_scope};
use unary_prefix::stepped_numeric_type;

pub(crate) use literal_text::{
    infer_array_literal_type, infer_type_from_constant_value,
    infer_type_from_constant_value_resolved,
};
pub(in crate::type_engine) use scope_reads::resolve_var_types;
pub(crate) use unary_prefix::unary_prefix_result_type;

pub(crate) use array_access::{class_string_inner_binding, insert_or_union, offset_read_type};
pub(crate) use assignment_value::{
    assignment_value, coalesce_assign_value, compound_assignment_result,
};
pub(crate) use branches::static_condition_truthiness;
pub(crate) use calls::{
    ArgWalkerTypes, build_function_template_subs, first_class_callable_return_type,
    infer_closure_literal_type, is_array_like_wrapper, resolve_arg_call_raw_type,
    resolve_arg_iterable_raw_type, resolve_arg_variable_raw_type, substitute_function_templates,
    walker_arg_types,
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

/// A closure value of type `closure_ty` (`Closure(): T` or bare `Closure`).
///
/// It is always resolved against the plain `Closure` class so that methods
/// like `bindTo()` are available for completion, with the typed callable
/// kept as the hint so invoking it still reads its signature.
fn closure_value(closure_ty: PhpType, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &PhpType::closure(),
        &ctx.current_class.fqn(),
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    if classes.is_empty() {
        vec![ResolvedType::from_type_string(closure_ty)]
    } else {
        ResolvedType::from_classes_with_hint(classes, closure_ty)
    }
}

/// Create a `ResolvedType` from a `PhpType`, looking up class info when the type names a class.
///
/// When the `PhpType` has a `base_name()` that resolves to a known class, returns
/// `ResolvedType::from_both(ty, class)`. Otherwise returns `ResolvedType::from_type_string(ty)`.
fn resolved_type_with_lookup(
    ty: PhpType,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> ResolvedType {
    if let Some(base) = ty.base_name() {
        let base = base.strip_prefix('\\').unwrap_or(base);
        // Don't try to look up scalars/pseudo-types
        if !crate::php_type::is_keyword_type(base)
            && let Some(class) =
                crate::class_lookup::find_or_load_resolved(base, all_classes, class_loader)
        {
            return ResolvedType::from_both_arc(ty, class);
        }
    }
    ResolvedType::from_type_string(ty)
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
            if let Some(cls) = ctx.file.all_classes.iter().find(|c| c.name == name) {
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
            if let Some(key) =
                crate::type_engine::type_ops::condition_facts::expr_to_subject_key(expr)
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
                && let Some(key) =
                    crate::type_engine::type_ops::condition_facts::expr_to_subject_key(expr)
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
            if let Some(key) =
                crate::type_engine::type_ops::condition_facts::expr_to_subject_key(expr)
                && (key.contains("->") || key.contains("::$"))
                && let Some(from_scope) = narrowed_subject_from_scope(&key, expr, ctx)
            {
                return from_scope;
            }
            resolve_rhs_property_access(access, ctx)
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
        // Type casts (`(int) $x`), `!`, `~`, and the signs.  The result
        // depends on the operator, not on how the expression is reached, so
        // a cast in a ternary branch resolves the same as one assigned
        // directly.  The signs are separate AST nodes rather than part of
        // numeric literals, so `-1` keeps its exact value through here.
        Expression::UnaryPrefix(unary) => {
            // A scalar cast checked earlier (`isset($map[(string) $k])`)
            // reads the entry the check left under the cast's key, the
            // way a function call does.
            if unary.operator.is_cast()
                && let Some(narrowed) = narrowed_call(expr, ctx)
            {
                return narrowed;
            }
            match unary_prefix_result_type(
                &unary.operator,
                unary.operand,
                || resolve_rhs_expression(unary.operand, ctx),
                ctx.file.class_loader,
                ctx.resolved_class_cache,
            ) {
                Some(ty) => vec![ResolvedType::from_type_string(ty)],
                None => vec![],
            }
        }
        Expression::Match(match_expr) => resolve_match_expression(match_expr, ctx),
        // `clone $expr` copies an object of the type `$expr` has.
        Expression::Clone(clone_expr) => resolve_rhs_expression(clone_expr.object, ctx),
        // ── Pipe operator (PHP 8.5): `$expr |> $callable` ──
        // The pipe calls the callable with `$expr`, so it returns what the
        // callable does, however it is written: `trim(...)`,
        // `$obj->method(...)`, a closure, or a variable holding one.
        Expression::Pipe(pipe) => function_calls::callable_invocation(pipe.callable, expr, ctx),
        Expression::PartialApplication(_)
        | Expression::Closure(_)
        | Expression::ArrowFunction(_) => {
            // Closures produce a `Closure` instance at runtime, but when we
            // can infer their body return type (explicit `: T`, generator
            // yields, or arrow-body expression), preserve it in the
            // `TypeKind::Callable` so callers like template binding can use
            // it through `$closure` variables.
            closure_value(infer_closure_literal_type(expr, ctx), ctx)
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
                        ctx.file.all_classes,
                        ctx.file.class_loader,
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
        // literal PHP would build (`'1' . 'a'` is `'1a'`). Otherwise a
        // non-empty operand makes the result non-empty (`'x' . $u`).
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            let foldable = scalar_fold::is_cheap_scalar_operand(binary.lhs)
                && scalar_fold::is_cheap_scalar_operand(binary.rhs);
            let lhs = resolve_rhs_expression(binary.lhs, ctx);
            if !foldable {
                return vec![ResolvedType::from_type_string(scalar_fold::concat_result(
                    &lhs,
                    || resolve_rhs_expression(binary.rhs, ctx),
                ))];
            }
            let rhs = resolve_rhs_expression(binary.rhs, ctx);
            let folded = scalar_fold::single_scalar(&lhs)
                .zip(scalar_fold::single_scalar(&rhs))
                .and_then(|(l, r)| scalar_fold::fold_concat(&l, &r));
            vec![ResolvedType::from_type_string(
                folded.unwrap_or_else(|| scalar_fold::concat_result(&lhs, || rhs)),
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
        Expression::ConstantAccess(ca) => resolve_rhs_constant_access(ca, ctx),
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
