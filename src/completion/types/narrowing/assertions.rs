//! Custom assertion narrowing: `@phpstan-assert` / `@psalm-assert`
//! method and function type guards, not-null assertions, and the
//! machinery that resolves assertion template types.

use std::sync::Arc;

use crate::atom::{Atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::types::{AssertionKind, ClassInfo, ParameterInfo, TypeAssertion};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::super::conditional::extract_class_string_from_expr;
use crate::completion::resolver::VarResolutionCtx;

use super::*;

/// Resolved assertion metadata extracted from a function call or static
/// method call expression.
///
/// Produced by [`extract_call_assertions`] so that callers can apply
/// narrowing logic uniformly regardless of whether the call is
/// `myFunc($x)` or `Assert::check($x)`.
pub(in crate::completion) struct CallAssertionInfo<'a> {
    /// The `@phpstan-assert` / `@psalm-assert` annotations on the callee.
    pub(in crate::completion) assertions: &'a [TypeAssertion],
    /// The callee's parameter list (used to map assertion `$param` names
    /// to positional argument indices).
    pub(in crate::completion) parameters: &'a [ParameterInfo],
    /// The call-site argument list.
    pub(in crate::completion) argument_list: &'a ArgumentList<'a>,
    /// Template parameter names from the callee's `@template` tags.
    template_params: &'a [Atom],
    /// Template parameter → parameter name bindings (e.g. `("T", "$class")`).
    template_bindings: &'a [(Atom, Atom)],
}

/// Try to extract assertion metadata from a call expression.
///
/// Handles two call forms:
///   - `Call::Function(func_call)` — standalone function call, resolved
///     through `ctx.function_loader`.
///   - `Call::StaticMethod(static_call)` — static method call like
///     `Assert::instanceOf(…)`, resolved through `ctx.class_loader`.
///
/// Returns `None` when the call is not one of these forms, or when the
/// callee cannot be resolved.
pub(in crate::completion) fn extract_call_assertions<'a>(
    call: &'a Call<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallAssertionInfo<'a>> {
    match call {
        Call::Function(func_call) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return None,
            };
            let func_name_offset = func_call.function.span().start.offset;
            let func_info = ctx.function_loader()?(&func_name, func_name_offset)?;
            if func_info.type_assertions.is_empty() {
                return None;
            }
            // SAFETY: We leak the FunctionInfo to get a stable reference.
            // This is acceptable because narrowing runs once per completion
            // request and the allocation is small.
            let func_info = Box::leak(Box::new(func_info));
            Some(CallAssertionInfo {
                assertions: &func_info.type_assertions,
                parameters: &func_info.parameters,
                argument_list: &func_call.argument_list,
                template_params: &func_info.template_params,
                template_bindings: &func_info.template_bindings,
            })
        }
        Call::StaticMethod(static_call) => {
            let method_name = match &static_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
                _ => return None,
            };
            let class_info = resolve_static_receiver_class(static_call.class, ctx)?;
            build_method_assertion_info(&class_info, method_name, &static_call.argument_list, ctx)
        }
        Call::Method(method_call) => {
            let method_name = match &method_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
                _ => return None,
            };
            let class_info = resolve_instance_receiver_class(method_call.object, ctx)?;
            build_method_assertion_info(&class_info, method_name, &method_call.argument_list, ctx)
        }
        Call::NullSafeMethod(method_call) => {
            let method_name = match &method_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
                _ => return None,
            };
            let class_info = resolve_instance_receiver_class(method_call.object, ctx)?;
            build_method_assertion_info(&class_info, method_name, &method_call.argument_list, ctx)
        }
    }
}

/// Resolve the receiver class of a static method call (the `X` in
/// `X::method()`) to a loaded [`ClassInfo`].
///
/// Handles class-name identifiers (including subclass names), `self`,
/// `static`, and `parent`.  The returned class is the raw parsed class;
/// callers resolve inheritance separately so that methods declared on an
/// ancestor (e.g. PHPUnit's `Assert::assertInstanceOf`) are found.
fn resolve_static_receiver_class(
    class_expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    match class_expr {
        Expression::Identifier(ident) => {
            let name = bytes_to_str(ident.value());
            let fqn = crate::util::resolve_name_via_loader(name, ctx.class_loader);
            (ctx.class_loader)(&fqn).or_else(|| (ctx.class_loader)(name))
        }
        Expression::Self_(_) | Expression::Static(_) => (ctx.class_loader)(&ctx.current_class.name),
        Expression::Parent(_) => {
            let parent = ctx.current_class.parent_class.as_ref()?;
            (ctx.class_loader)(parent)
        }
        _ => None,
    }
}

/// Resolve the receiver class of an instance method call (the `$x` in
/// `$x->method()`) to a loaded [`ClassInfo`].
///
/// `$this` resolves to the enclosing class.  Other variables are resolved
/// through the forward walker's scope so that, for example,
/// `$test->assertInstanceOf(...)` narrows correctly.
fn resolve_instance_receiver_class(
    object_expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    let Expression::Variable(Variable::Direct(dv)) = object_expr else {
        return None;
    };
    // Variable names carry the leading `$` (e.g. `$this`, `$obj`).
    let name = bytes_to_str(dv.name);
    if name == "$this" {
        return (ctx.class_loader)(&ctx.current_class.name);
    }
    let resolver = ctx.scope_var_resolver?;
    let first = resolver(name).into_iter().next()?;
    (ctx.class_loader)(&first.type_string.to_string())
}

/// Build [`CallAssertionInfo`] for a method call once the receiver class
/// has been resolved.
///
/// Walks the receiver's trait and parent chain (using raw class loads) so
/// that assertion annotations declared on an ancestor are found — e.g.
/// PHPUnit's `assertInstanceOf`, declared on the base `Assert` class and
/// called through a `TestCase` subclass.  Returns `None` when no
/// reachable definition of the method carries assertions.
///
/// A full inheritance merge is deliberately avoided here: this runs inside
/// the forward walker while the enclosing class may itself be mid-resolution,
/// and `resolve_class_fully` would write a partial result into the shared
/// resolved-class cache, corrupting later member lookups.
fn build_method_assertion_info<'a>(
    class: &ClassInfo,
    method_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallAssertionInfo<'a>> {
    let method =
        find_assertion_method_in_chain(class, method_name, ctx.class_loader, &mut Vec::new(), 0)?;
    // Leak MethodInfo to get a stable reference for the duration of this
    // narrowing pass.
    let method = Box::leak(Box::new(method));
    Some(CallAssertionInfo {
        assertions: &method.type_assertions,
        parameters: &method.parameters,
        argument_list,
        template_params: &method.template_params,
        template_bindings: &method.template_bindings,
    })
}

/// Find the definition of `method_name` that carries `@phpstan-assert`
/// metadata, searching the class's own methods, its traits, and its parent
/// chain (in PHP resolution order).  Uses raw class loads only, so it never
/// mutates the shared resolved-class cache.
///
/// Returns an owned clone of the first matching method that has non-empty
/// `type_assertions`.  A `visited` set and `depth` bound guard against
/// cyclic hierarchies.
pub(in crate::completion) fn find_assertion_method_in_chain(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut Vec<Atom>,
    depth: usize,
) -> Option<crate::types::MethodInfo> {
    if depth > 15 {
        return None;
    }
    let fqn = class.fqn();
    if visited.contains(&fqn) {
        return None;
    }
    visited.push(fqn);

    // Own methods first: the most-derived definition wins.  A derived
    // override with its own assertions takes precedence; an override with
    // no docblock falls through so an ancestor's assertions can apply
    // (matching how inheritance propagates assertion metadata).
    if let Some(method) = class
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(method_name))
        && !method.type_assertions.is_empty()
    {
        return Some(method.as_ref().clone());
    }

    // Traits mixed into this class.
    for trait_name in &class.used_traits {
        if let Some(trait_class) = class_loader(trait_name)
            && let Some(method) = find_assertion_method_in_chain(
                &trait_class,
                method_name,
                class_loader,
                visited,
                depth + 1,
            )
        {
            return Some(method);
        }
    }

    // Parent class chain.
    if let Some(parent) = class.parent_class.as_ref()
        && let Some(parent_class) = class_loader(parent)
        && let Some(method) = find_assertion_method_in_chain(
            &parent_class,
            method_name,
            class_loader,
            visited,
            depth + 1,
        )
    {
        return Some(method);
    }

    None
}

/// Apply narrowing from `@phpstan-assert` / `@psalm-assert` annotations
/// on a function or static method called as a standalone expression statement.
///
/// Only `AssertionKind::Always` assertions are applied here — the
/// `IfTrue` / `IfFalse` variants are handled by
/// `try_apply_assert_condition_narrowing`.
///
/// Map a bare scalar / pseudo-type to the type-guard kind that narrows it.
///
/// So `@phpstan-assert string $x` (PHPUnit's `assertIsString`) narrows like
/// `is_string($x)`, and its negation excludes `string`.  Returns `None` for
/// class names and for pseudo-types without a corresponding guard —
/// `iterable`, `resource`, and `null` (the last handled separately by the
/// not-null path) — so those fall through to the class-based narrowing.
fn scalar_assert_guard_kind(ty: &PhpType) -> Option<TypeGuardKind> {
    match ty {
        PhpType::Array(_) | PhpType::ArrayShape(_) => Some(TypeGuardKind::Array),
        PhpType::Generic(name, _) if crate::php_type::is_array_like_name(name) => {
            // `iterable` is array-like by name but has no `is_iterable` guard
            // kind, so it must not map to the array guard.
            (!name.eq_ignore_ascii_case("iterable")).then_some(TypeGuardKind::Array)
        }
        PhpType::Named(n) => match n.to_ascii_lowercase().as_str() {
            "array" | "list" | "non-empty-array" | "non-empty-list" => Some(TypeGuardKind::Array),
            "string" => Some(TypeGuardKind::String),
            "int" | "integer" => Some(TypeGuardKind::Int),
            "float" | "double" => Some(TypeGuardKind::Float),
            "bool" | "boolean" => Some(TypeGuardKind::Bool),
            "object" => Some(TypeGuardKind::Object),
            "numeric" => Some(TypeGuardKind::Numeric),
            "callable" => Some(TypeGuardKind::Callable),
            "scalar" => Some(TypeGuardKind::Scalar),
            _ => None,
        },
        _ => None,
    }
}

/// Scalar and pseudo-type assertions (PHPUnit's `assertIsString`,
/// `assertIsObject`, `assertIsArray`, and their negations) name no class, so
/// they cannot be narrowed through `apply_instanceof_*`.  When one is
/// detected, `*type_guard` is set to `(kind, exclude)` and the caller applies
/// [`apply_type_guard_inclusion`] / [`apply_type_guard_exclusion`] on the full
/// resolved types instead, matching how the corresponding `is_*()` guard
/// narrows.  The same channel carries the `object` fallback for a template
/// assertion whose bound `class-string` argument could not be resolved
/// (e.g. `assertInstanceOf($variableClass, $x)`): the subject is still known
/// to be an object, so it is narrowed to `object` rather than cleared.
///
/// Returns `true` when a definite (inclusion-style) narrowing was
/// applied to `results` — see [`ResolvedType::apply_narrowing`]. The
/// scalar/pseudo-type and template-deferral branches signal through
/// `type_guard` instead and do not affect `results` here, so they
/// contribute `false`.
pub(in crate::completion) fn try_apply_custom_assert_narrowing(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    type_guard: &mut Option<(TypeGuardKind, bool)>,
) -> bool {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let call = match expr {
        Expression::Call(c) => c,
        _ => return false,
    };
    let info = match extract_call_assertions(call, ctx) {
        Some(info) => info,
        None => return false,
    };
    let mut definite = false;
    for assertion in info.assertions {
        if assertion.kind != AssertionKind::Always {
            continue;
        }
        if let Some(arg_var) =
            find_assertion_arg_variable(info.argument_list, &assertion.param_name, info.parameters)
            && arg_var == ctx.var_name
        {
            // Resolve the asserted type.  When the type is a template
            // parameter (e.g. `ExpectedType` from `@phpstan-assert
            // ExpectedType $actual`), substitute it using the call-site
            // argument bound via `class-string<T>`.
            let effective_type =
                resolve_assertion_template_type(&assertion.asserted_type, &info, ctx);

            // The substitution failed when the effective type is still a
            // template parameter — the bound `class-string` argument was a
            // variable whose concrete class could not be determined.  A
            // positive assertion still guarantees the subject is an object,
            // so defer to the caller's `object` narrowing instead of
            // clearing the subject's prior type.
            if !assertion.negated
                && matches!(&effective_type, PhpType::Named(n) if info.template_params.iter().any(|t| t == n))
            {
                *type_guard = Some((TypeGuardKind::Object, false));
                continue;
            }

            // Scalar / pseudo-type assertions (`assertIsString`,
            // `assertIsObject`, `assertIsArray`, and their `assertIsNot*`
            // negations) are type guards, not class narrowings.  The named
            // pseudo-type resolves to no class, so `apply_instanceof_inclusion`
            // would clear the subject and `apply_instanceof_exclusion` would
            // exclude nothing.  Route them through the type-guard machinery.
            if let Some(kind) = scalar_assert_guard_kind(&effective_type) {
                *type_guard = Some((kind, assertion.negated));
                continue;
            }

            if assertion.negated {
                apply_instanceof_exclusion(&effective_type, ctx, results);
            } else {
                definite |= apply_instanceof_inclusion(&effective_type, false, ctx, results);
            }
        }
    }
    definite
}

/// Collect argument expressions that an assert-style call proves to be
/// `true` or `false` by re-exporting an inner condition.
///
/// PHPUnit's `assertTrue()` carries `@phpstan-assert true $condition` and
/// `assertFalse()` carries `@phpstan-assert false $condition` (the
/// `@psalm-assert` spelling is treated identically).  When the matching
/// argument is itself a boolean condition expression (e.g.
/// `property_exists($model, 'value')`), asserting that it is `true` /
/// `false` is equivalent to entering an `if` guarded by that condition.
///
/// Returns each such argument expression paired with the polarity the
/// assertion proves: `true` means the expression is proven true (apply
/// truthy condition narrowing), `false` means proven false (apply the
/// inverse).  The caller feeds each expression into the standard
/// condition-narrowing pipeline so every guard form (`instanceof`,
/// `is_*`, `property_exists`, null checks, …) is honoured uniformly.
pub(in crate::completion) fn collect_assert_reexport_conditions<'a>(
    expr: &'a Expression<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<(&'a Expression<'a>, bool)> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let Expression::Call(call) = expr else {
        return Vec::new();
    };
    let Some(info) = extract_call_assertions(call, ctx) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for assertion in info.assertions {
        if assertion.kind != AssertionKind::Always {
            continue;
        }
        // Only a bare `true` / `false` literal assertion re-exports a
        // condition.  `@phpstan-assert true $c` (negated `!true` ⇒ false)
        // proves the argument true; `@phpstan-assert false $c` proves it
        // false.
        let asserts_true = if assertion.asserted_type.is_true() {
            !assertion.negated
        } else if assertion.asserted_type.is_false() {
            assertion.negated
        } else {
            continue;
        };
        if let Some(arg_expr) =
            assertion_arg_expression(info.argument_list, &assertion.param_name, info.parameters)
        {
            out.push((arg_expr, asserts_true));
        }
    }
    out
}

/// Return the call-site argument expression bound to `param_name`.
///
/// Unlike [`find_assertion_arg_variable`], which reduces the argument to a
/// subject key (and so discards non-subject expressions like nested
/// calls), this returns the raw expression so the caller can treat it as a
/// re-exported condition.
fn assertion_arg_expression<'a>(
    argument_list: &'a ArgumentList<'a>,
    param_name: &str,
    parameters: &[crate::types::ParameterInfo],
) -> Option<&'a Expression<'a>> {
    let param_idx = parameters.iter().position(|p| p.name == param_name)?;
    let arg = argument_list.arguments.iter().nth(param_idx)?;
    Some(match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    })
}

/// Report whether a call expression carries an unconditional not-null
/// assertion (`@phpstan-assert !null $param`, e.g. PHPUnit's
/// `assertNotNull`) whose argument resolves to `ctx.var_name`.
///
/// The class-based [`apply_instanceof_exclusion`] cannot remove the `null`
/// pseudo-type (it isn't a class), so callers use this to strip `null` from
/// a subject's [`ResolvedType`] list directly.  Returns `true` when such an
/// assertion applies to the current subject.
pub(in crate::completion) fn call_asserts_not_null(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> bool {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let Expression::Call(call) = expr else {
        return false;
    };
    let Some(info) = extract_call_assertions(call, ctx) else {
        return false;
    };
    info.assertions.iter().any(|assertion| {
        assertion.kind == AssertionKind::Always
            && assertion.negated
            && assertion.asserted_type.is_null()
            && find_assertion_arg_variable(
                info.argument_list,
                &assertion.param_name,
                info.parameters,
            )
            .as_deref()
                == Some(ctx.var_name)
    })
}

/// If `asserted_type` is a template parameter name, resolve it to a
/// concrete type using the call-site arguments and template bindings.
///
/// For example, given:
///   `@template ExpectedType of object`
///   `@param class-string<ExpectedType> $expected`
///   `@phpstan-assert ExpectedType $actual`
///   Call: `Assert::assertFoobar(Foobar::class, $obj)`
///
/// The asserted type `ExpectedType` is resolved to `Foobar` by:
///   1. Finding `ExpectedType` in `template_params`
///   2. Looking up its binding: `("ExpectedType", "$expected")`
///   3. Finding positional index of `$expected` in `parameters`
///   4. Reading the call-site argument at that index: `Foobar::class`
///   5. Extracting the class name `Foobar`
///
/// Returns the original type unchanged when it is not a template param
/// or when the concrete type cannot be determined.
fn resolve_assertion_template_type(
    asserted_type: &PhpType,
    info: &CallAssertionInfo<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    // Check if the asserted type is a template parameter.
    let tpl_name = match asserted_type {
        PhpType::Named(n) if info.template_params.iter().any(|t| t == n) => n.as_str(),
        _ => return asserted_type.clone(),
    };

    // Find the parameter name that binds this template param.
    let bound_param = info
        .template_bindings
        .iter()
        .find(|(tpl, _)| tpl == tpl_name)
        .map(|(_, param)| param.as_str());

    let bound_param = match bound_param {
        Some(p) => p,
        None => return asserted_type.clone(),
    };

    // Find the positional index of that parameter.
    let param_idx = match info.parameters.iter().position(|p| p.name == bound_param) {
        Some(idx) => idx,
        None => return asserted_type.clone(),
    };

    // Get the call-site argument at that position.
    let arg_expr = match info.argument_list.arguments.iter().nth(param_idx) {
        Some(Argument::Positional(pos)) => pos.value,
        Some(Argument::Named(named)) => named.value,
        None => return asserted_type.clone(),
    };

    // Try to extract a class name from the argument expression.
    if let Some(class_name) = extract_class_string_from_expr(arg_expr) {
        let fqn = crate::util::resolve_name_via_loader(&class_name, ctx.class_loader);
        return PhpType::Named(fqn);
    }

    if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
        let var_name = bytes_to_str(dv.name).to_string();

        // Prefer the shared forward walker's tracked type for the variable.
        // When the walker is driving this narrowing it has already processed
        // the statements leading up to the assert, so a variable holding a
        // `class-string<Wanted>` value (whether assigned directly, via
        // null-coalesce, or list-destructured out of a foreach source array)
        // is in scope with that type.  Reusing it keeps class-string-value
        // resolution on the single shared pipeline instead of a parallel
        // special-purpose walk that only recognizes direct assignments.
        if let Some(scope_resolver) = ctx.scope_var_resolver {
            for resolved in scope_resolver(&var_name) {
                if let Some(PhpType::Named(name)) = resolved.type_string.unwrap_class_string_inner()
                {
                    return PhpType::Named(name.clone());
                }
            }
        }

        // Fall back to the class-string resolver for consumers without a live
        // forward-walk scope (e.g. a completion request resolving the subject
        // directly).  Resolve it at the argument's own offset rather than
        // `ctx.cursor_offset`: the latter is `u32::MAX` during whole-method
        // diagnostics walks, which defeats the class-body detection in
        // `resolve_class_string_targets` (its `cursor <= class_end` bound never
        // holds), and using the call site is more precise anyway (a later
        // reassignment of the variable must not fold back into the assertion).
        let targets =
            crate::completion::variable::class_string_resolution::resolve_class_string_targets(
                &var_name,
                ctx.current_class,
                ctx.all_classes,
                ctx.content,
                arg_expr.span().start.offset,
                ctx.class_loader,
            );
        if let Some(first) = targets.into_iter().next() {
            return PhpType::Named(first.name.to_string());
        }
    }

    asserted_type.clone()
}

/// Unwrap parentheses and a single `!` prefix from a condition,
/// returning `(inner_expr, negated)`.
pub(in crate::completion) fn unwrap_condition_negation<'b>(
    expr: &'b Expression<'b>,
) -> (&'b Expression<'b>, bool) {
    match expr {
        Expression::Parenthesized(inner) => unwrap_condition_negation(inner.expression),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            let (inner, already_negated) = unwrap_condition_negation(prefix.operand);
            (inner, !already_negated)
        }
        _ => (expr, false),
    }
}

/// Given a function's argument list and a parameter name (with `$`
/// prefix), find the subject key passed at that parameter's position.
///
/// Returns the subject key for a direct variable (`$var`), a property
/// path (`$arg->value`), or an array access (`$stmts["0"]`) so that
/// assertion narrowing applies to non-variable subjects, not just plain
/// variables.
pub(in crate::completion) fn find_assertion_arg_variable(
    argument_list: &ArgumentList<'_>,
    param_name: &str,
    parameters: &[crate::types::ParameterInfo],
) -> Option<String> {
    // Find the parameter index
    let param_idx = parameters.iter().position(|p| p.name == param_name)?;

    // Get the argument at that position
    let arg = argument_list.arguments.iter().nth(param_idx)?;
    let arg_expr = match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };

    expr_to_subject_key(arg_expr)
}
