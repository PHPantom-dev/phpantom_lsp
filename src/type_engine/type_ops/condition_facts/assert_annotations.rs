//! Custom assertion narrowing: `@phpstan-assert` / `@psalm-assert`
//! method and function type guards, not-null assertions, and the
//! machinery that resolves assertion template types.

use std::sync::Arc;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{AssertionKind, ClassInfo, ParameterInfo, SharedVec, TypeAssertion};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::super::conditional::extract_class_string_from_expr;
use crate::type_engine::resolver::VarResolutionCtx;

use super::*;

/// Resolved assertion metadata extracted from a function call or static
/// method call expression.
///
/// Produced by [`extract_call_assertions`] so that callers can apply
/// narrowing logic uniformly regardless of whether the call is
/// `myFunc($x)` or `Assert::check($x)`.
///
/// The callee metadata is owned rather than borrowed: the callee is a
/// clone produced by the loader (or by the trait/parent chain walk), so
/// there is nothing outliving the call to borrow from. Moving the four
/// fields out of that clone keeps the cost to an `Arc` bump plus three
/// `Vec` moves, and lets the rest of the clone drop immediately.
pub(in crate::type_engine) struct CallAssertionInfo<'a> {
    /// The `@phpstan-assert` / `@psalm-assert` annotations on the callee.
    pub(in crate::type_engine) assertions: Vec<TypeAssertion>,
    /// The callee's parameter list (used to map assertion `$param` names
    /// to positional argument indices).
    pub(in crate::type_engine) parameters: SharedVec<ParameterInfo>,
    /// The call-site argument list.
    pub(in crate::type_engine) argument_list: &'a ArgumentList<'a>,
    /// The subject key of the object the call was made on (`"$this"`,
    /// `"$reflection"`, …), for a method call whose receiver is a plain
    /// variable.
    ///
    /// An assertion tag may name a path through the receiver rather than
    /// an argument — `@phpstan-assert bool $this->resolved` on a method
    /// promises something about the object it was called on. Reading the
    /// tag at the call site means substituting this for its `$this`.
    pub(in crate::type_engine) receiver_key: Option<String>,
    /// Template parameter names from the callee's `@template` tags.
    template_params: Vec<Atom>,
    /// Template parameter → parameter name bindings (e.g. `("T", "$class")`).
    template_bindings: Vec<(Atom, Atom)>,
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
pub(in crate::type_engine) fn extract_call_assertions<'a>(
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
            Some(CallAssertionInfo {
                assertions: func_info.type_assertions,
                parameters: func_info.parameters,
                argument_list: &func_call.argument_list,
                receiver_key: None,
                template_params: func_info.template_params,
                template_bindings: func_info.template_bindings,
            })
        }
        Call::StaticMethod(static_call) => {
            let method_name = match &static_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
                _ => return None,
            };
            let class_info = resolve_static_receiver_class(static_call.class, ctx)?;
            build_method_assertion_info(
                &class_info,
                method_name,
                &static_call.argument_list,
                None,
                ctx,
            )
        }
        // `->` and `?->` reach the same method; only the receiver's
        // nullability differs, which says nothing about its assertions.
        Call::Method(MethodCall {
            object,
            method,
            argument_list,
            ..
        })
        | Call::NullSafeMethod(NullSafeMethodCall {
            object,
            method,
            argument_list,
            ..
        }) => instance_method_assertion_info(object, method, argument_list, ctx),
    }
}

/// Resolve the receiver class of a static method call (the `X` in
/// `X::method()`) to a loaded [`ClassInfo`].
///
/// Handles class-name identifiers (including subclass names), `self`,
/// `static`, and `parent`.  The returned class is the raw parsed class;
/// callers resolve inheritance separately so that methods declared on an
/// ancestor (e.g. PHPUnit's `Assert::assertInstanceOf`) are found.
pub(super) fn resolve_static_receiver_class(
    class_expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    ctx.written_names().static_receiver_class(class_expr)
}

/// Resolve the receiver class of an instance method call (the `$x` in
/// `$x->method()`) to a loaded [`ClassInfo`].
///
/// `$this` resolves to the enclosing class.  Other variables are resolved
/// through the forward walker's scope so that, for example,
/// `$test->assertInstanceOf(...)` narrows correctly.
pub(super) fn resolve_instance_receiver_class(
    object_expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    let Expression::Variable(Variable::Direct(dv)) = object_expr else {
        return None;
    };
    // Variable names carry the leading `$` (e.g. `$this`, `$obj`).
    let name = bytes_to_str(dv.name);
    if name == "$this" {
        return (ctx.file.class_loader)(Fqn::new(&ctx.current_class.fqn()));
    }
    let resolver = ctx.scope_var_resolver?;
    let first = resolver(name).into_iter().next()?;
    (ctx.file.class_loader)(Fqn::new(&first.type_string.to_string()))
}

/// The assertions an instance method call carries, for either arrow.
fn instance_method_assertion_info<'a>(
    object: &Expression<'_>,
    selector: &ClassLikeMemberSelector<'_>,
    argument_list: &'a ArgumentList<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallAssertionInfo<'a>> {
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return None;
    };
    let class_info = resolve_instance_receiver_class(object, ctx)?;
    build_method_assertion_info(
        &class_info,
        bytes_to_str(ident.value),
        argument_list,
        expr_to_subject_key(object),
        ctx,
    )
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
    receiver_key: Option<String>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallAssertionInfo<'a>> {
    let (method, _) = find_assertion_method_in_chain(
        class,
        method_name,
        ctx.file.class_loader,
        &mut Vec::new(),
        0,
    )?;
    let extras = method.extras.as_deref();
    Some(CallAssertionInfo {
        assertions: extras
            .map(|e| e.type_assertions.clone())
            .unwrap_or_default(),
        parameters: method.parameters.clone(),
        argument_list,
        receiver_key,
        template_params: extras
            .map(|e| e.template_params.clone())
            .unwrap_or_default(),
        template_bindings: extras
            .map(|e| e.template_bindings.clone())
            .unwrap_or_default(),
    })
}

/// Find the definition of `method_name` that carries `@phpstan-assert`
/// metadata, searching the class's own methods, its traits, and its parent
/// chain (in PHP resolution order).  Uses raw class loads only, so it never
/// mutates the shared resolved-class cache.
///
/// Returns the first matching method that has non-empty `type_assertions`,
/// paired with the FQN of the class that declared it.
/// The declaring class is what an unqualified name in the tag resolves
/// against, which is not necessarily the receiver's own class.  A
/// `visited` set and `depth` bound guard against cyclic hierarchies.
pub(in crate::type_engine) fn find_assertion_method_in_chain(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
    visited: &mut Vec<Atom>,
    depth: usize,
) -> Option<(Arc<crate::types::MethodInfo>, Atom)> {
    find_method_in_chain_where(
        class,
        method_name,
        class_loader,
        &|m| !m.type_assertions().is_empty(),
        visited,
        depth,
    )
}

/// Rewrite a definition found on `ancestor` in terms of `class`, which
/// binds the ancestor's templates through `@extends` / `@implements`.
///
/// The walk loads classes raw, so a tag written against the ancestor's own
/// template (`@phpstan-assert-if-true T $id` on `Fetcher<T>`) reaches the
/// call site still naming `T`; `PostFetcher`'s `@implements
/// Fetcher<PostId>` is what says it means `PostId` there.
fn bind_ancestor_templates(
    class: &ClassInfo,
    ancestor: &ClassInfo,
    (method, declaring): (Arc<crate::types::MethodInfo>, Atom),
) -> (Arc<crate::types::MethodInfo>, Atom) {
    if !ancestor.template_params.is_empty() {
        let subs = crate::inheritance::build_substitution_map(class, ancestor, &Default::default());
        if !subs.is_empty() {
            let mut method = Arc::unwrap_or_clone(method);
            crate::inheritance::apply_substitution_to_method(&mut method, &subs);
            return (Arc::new(method), declaring);
        }
    }
    (method, declaring)
}

/// [`find_assertion_method_in_chain`] generalised over what makes a
/// definition the interesting one: assertion tags for the assert
/// narrowing, a conditional return type for the `never`-branch one.
///
/// The second element of the result is the FQN of the class the returned
/// definition was found on.
pub(in crate::type_engine) fn find_method_in_chain_where(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
    carries_metadata: &dyn Fn(&crate::types::MethodInfo) -> bool,
    visited: &mut Vec<Atom>,
    depth: usize,
) -> Option<(Arc<crate::types::MethodInfo>, Atom)> {
    if depth > 15 {
        return None;
    }
    let fqn = class.fqn();
    if visited.contains(&fqn) {
        return None;
    }
    visited.push(fqn);

    // Own methods first: the most-derived definition wins.  A derived
    // override with its own metadata takes precedence; an override with
    // no docblock falls through so an ancestor's can apply (matching how
    // inheritance propagates this metadata).
    if let Some(method) = class
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(method_name))
        && carries_metadata(method)
    {
        return Some((Arc::clone(method), fqn));
    }

    // Traits mixed into this class.
    for trait_name in &class.used_traits {
        if let Some(trait_class) = class_loader(Fqn::new(trait_name))
            && let Some(method) = find_method_in_chain_where(
                &trait_class,
                method_name,
                class_loader,
                carries_metadata,
                visited,
                depth + 1,
            )
        {
            return Some(method);
        }
    }

    // Parent class chain.
    if let Some(parent) = class.parent_class.as_ref()
        && let Some(parent_class) = class_loader(Fqn::new(parent))
        && let Some(found) = find_method_in_chain_where(
            &parent_class,
            method_name,
            class_loader,
            carries_metadata,
            visited,
            depth + 1,
        )
    {
        return Some(bind_ancestor_templates(class, &parent_class, found));
    }

    // Implemented interfaces, last: a class that redeclares the method
    // without a docblock inherits the contract's, which is where a
    // predicate's `@phpstan-assert` tags normally live (`Scope` declares
    // them and `MutatingScope` implements them).
    for interface_name in &class.interfaces {
        if let Some(interface) = class_loader(Fqn::new(interface_name))
            && let Some(found) = find_method_in_chain_where(
                &interface,
                method_name,
                class_loader,
                carries_metadata,
                visited,
                depth + 1,
            )
        {
            return Some(bind_ancestor_templates(class, &interface, found));
        }
    }

    None
}

/// Map a bare scalar / pseudo-type to the type-guard kind that narrows it.
///
/// So `@phpstan-assert string $x` (PHPUnit's `assertIsString`) narrows like
/// `is_string($x)`, and its negation excludes `string`.  Returns `None` for
/// class names, so those fall through to the class-based narrowing.
pub(in crate::type_engine) fn scalar_assert_guard_kind(ty: &PhpType) -> Option<TypeGuardKind> {
    match ty.kind() {
        TypeKind::Array(_) | TypeKind::ArrayShape(_) => Some(TypeGuardKind::Array),
        TypeKind::Generic(g) if crate::php_type::is_array_like_name(&g.name) => {
            // `iterable<T>` is array-like by name but admits a `Traversable`
            // too, so it takes the wider guard.
            Some(if g.name.eq_ignore_ascii_case("iterable") {
                TypeGuardKind::Iterable
            } else {
                TypeGuardKind::Array
            })
        }
        TypeKind::Named(n) => match n.to_ascii_lowercase().as_str() {
            "array" | "list" | "non-empty-array" | "non-empty-list" => Some(TypeGuardKind::Array),
            "iterable" => Some(TypeGuardKind::Iterable),
            "string" => Some(TypeGuardKind::String),
            "int" | "integer" => Some(TypeGuardKind::Int),
            "float" | "double" => Some(TypeGuardKind::Float),
            "bool" | "boolean" => Some(TypeGuardKind::Bool),
            "object" => Some(TypeGuardKind::Object),
            "numeric" => Some(TypeGuardKind::Numeric),
            "callable" => Some(TypeGuardKind::Callable),
            "scalar" => Some(TypeGuardKind::Scalar),
            // `assertIsResource`, and the two states PHP's own stubs
            // spell a handle in once it has been opened or closed.
            "resource" | "open-resource" | "closed-resource" => Some(TypeGuardKind::Resource),
            "null" => Some(TypeGuardKind::Null),
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
/// `*intersected` is set when the assertion proved a class the subject
/// does not nominally implement — PHPUnit's `assertInstanceOf(Node::class,
/// $mock)` on a `MockObject`, where the value really is both at once. The
/// caller has to tag the surviving entries as an intersection; left as a
/// plain list they read as a union, which says the value is one *or* the
/// other and satisfies neither half's declared type.
///
/// Returns `true` when a definite (inclusion-style) narrowing was
/// applied to `results` — see [`ResolvedType::apply_narrowing`]. The
/// scalar/pseudo-type and template-deferral branches signal through
/// `type_guard` instead and do not affect `results` here, so they
/// contribute `false`.
pub(in crate::type_engine) fn try_apply_custom_assert_narrowing(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<Arc<ClassInfo>>,
    type_guard: &mut Option<(TypeGuardKind, bool)>,
    intersected: &mut bool,
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
    for assertion in &info.assertions {
        if assertion.kind != AssertionKind::Always {
            continue;
        }
        if let Some(arg_var) = assertion_subject_key(&assertion.param_name, &info)
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
                && matches!(&effective_type.kind(), TypeKind::Named(n) if info.template_params.iter().any(|t| t == n))
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
                // `apply_instanceof_inclusion` only *grows* a single
                // starting class into two entries through its "keep both"
                // branch — every other path clears and replaces — so
                // growth is the signal that the merge was an
                // intersection.
                let before = results.len();
                definite |= apply_instanceof_inclusion(&effective_type, false, ctx, results);
                *intersected |= before <= 1 && results.len() > before;
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
pub(in crate::type_engine) fn collect_assert_reexport_conditions<'a>(
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
    for assertion in &info.assertions {
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
            assertion_arg_expression(info.argument_list, &assertion.param_name, &info.parameters)
        {
            out.push((arg_expr, asserts_true));
        }
    }
    out
}

/// The class PHPUnit declares its assertions on.
const PHPUNIT_ASSERT_FQN: &str = "PHPUnit\\Framework\\Assert";

/// The check a PHPUnit assertion without `@phpstan-assert` proves.
///
/// PHPUnit declares no `@phpstan-assert` on these, so the name of a method
/// declared on `PHPUnit\Framework\Assert` is the only signal.  PHPStan's
/// PHPUnit extension reads them the same way, as the check each assertion
/// stands for.
pub(in crate::type_engine) enum PhpunitAssertion<'a> {
    /// `assertSame()` / `assertNotSame()`: `$expected === $actual`, or
    /// `!==` when not `identical`.
    Same {
        expected: &'a Expression<'a>,
        actual: &'a Expression<'a>,
        identical: bool,
    },
    /// `assertCount()` / `assertNotCount()`:
    /// `count($haystack) === $expected`, or `!==` when not `equal`.
    Count {
        expected: &'a Expression<'a>,
        haystack: &'a Expression<'a>,
        equal: bool,
    },
    /// `assertArrayHasKey()` / `assertArrayNotHasKey()`:
    /// `array_key_exists($key, $array)`, negated when not `present`.
    HasKey {
        key: &'a Expression<'a>,
        array: &'a Expression<'a>,
        present: bool,
    },
    /// `assertNotEmpty()` / `assertEmpty()`: `!empty($actual)`, which is
    /// `$actual` being truthy, or falsy when `empty`.
    Truthy {
        actual: &'a Expression<'a>,
        empty: bool,
    },
}

/// Recognise a call to one of the PHPUnit assertions in
/// [`PhpunitAssertion`].
pub(in crate::type_engine) fn phpunit_assertion<'a>(
    call: &'a Call<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpunitAssertion<'a>> {
    let (class_info, selector, argument_list) = match call {
        Call::StaticMethod(static_call) => (
            resolve_static_receiver_class(static_call.class, ctx)?,
            &static_call.method,
            &static_call.argument_list,
        ),
        Call::Method(MethodCall {
            object,
            method,
            argument_list,
            ..
        }) => (
            resolve_instance_receiver_class(object, ctx)?,
            method,
            argument_list,
        ),
        _ => return None,
    };
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return None;
    };
    let method_name = bytes_to_str(ident.value);
    let lower = method_name.to_ascii_lowercase();
    if !matches!(
        lower.as_str(),
        "assertsame"
            | "assertnotsame"
            | "assertcount"
            | "assertnotcount"
            | "assertarrayhaskey"
            | "assertarraynothaskey"
            | "assertempty"
            | "assertnotempty"
    ) {
        return None;
    }
    let (method, declaring) = find_method_in_chain_where(
        &class_info,
        method_name,
        ctx.file.class_loader,
        &|_| true,
        &mut Vec::new(),
        0,
    )?;
    if declaring != PHPUNIT_ASSERT_FQN {
        return None;
    }
    let arg = |name: &str| assertion_arg_expression(argument_list, name, &method.parameters);
    Some(match lower.as_str() {
        "assertsame" | "assertnotsame" => PhpunitAssertion::Same {
            expected: arg("$expected")?,
            actual: arg("$actual")?,
            identical: lower == "assertsame",
        },
        "assertcount" | "assertnotcount" => PhpunitAssertion::Count {
            expected: arg("$expectedCount")?,
            haystack: arg("$haystack")?,
            equal: lower == "assertcount",
        },
        "assertempty" | "assertnotempty" => PhpunitAssertion::Truthy {
            actual: arg("$actual")?,
            empty: lower == "assertempty",
        },
        _ => PhpunitAssertion::HasKey {
            key: arg("$key")?,
            array: arg("$array")?,
            present: lower == "assertarrayhaskey",
        },
    })
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
pub(in crate::type_engine) fn call_asserts_not_null(
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
                &info.parameters,
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
    let tpl_name = match asserted_type.kind() {
        TypeKind::Named(n) if info.template_params.iter().any(|t| t == n) => n.as_str(),
        _ => return asserted_type.clone(),
    };
    template_argument_at_call(
        tpl_name,
        &info.template_bindings,
        &info.parameters,
        info.argument_list,
        ctx,
    )
    .unwrap_or_else(|| asserted_type.clone())
}

/// `asserted` with each of the callee's own `@template` parameters replaced
/// by the class the call binds it to through a `class-string<T>` argument.
///
/// [`resolve_assertion_template_type`] answers the case where the tag names
/// the template bare; a tag can also name it inside another type
/// (`@phpstan-assert-if-true ReflectionClass<T> $this`), and the argument
/// binds it just the same.  A template the call leaves unbound stays as
/// written.
pub(in crate::type_engine) fn bind_call_templates(
    asserted: &PhpType,
    template_params: &[Atom],
    template_bindings: &[(Atom, Atom)],
    parameters: &[ParameterInfo],
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    if template_params.is_empty() {
        return asserted.clone();
    }
    let mut subs = std::collections::HashMap::new();
    for tpl in template_params {
        if !asserted.references_any_template_param(&[tpl.to_string()]) {
            continue;
        }
        if let Some(bound) =
            template_argument_at_call(tpl, template_bindings, parameters, argument_list, ctx)
        {
            subs.insert(tpl.to_string(), bound);
        }
    }
    if subs.is_empty() {
        asserted.clone()
    } else {
        asserted.substitute(&subs)
    }
}

/// The class a call binds the template `tpl_name` to, read off the
/// `class-string<T>` argument its binding names.
fn template_argument_at_call(
    tpl_name: &str,
    template_bindings: &[(Atom, Atom)],
    parameters: &[ParameterInfo],
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    // Find the parameter name that binds this template param.
    let bound_param = template_bindings
        .iter()
        .find(|(tpl, _)| tpl == tpl_name)
        .map(|(_, param)| param.as_str())?;

    // Find the positional index of that parameter.
    let param_idx = parameters.iter().position(|p| p.name == bound_param)?;

    // Get the call-site argument at that position.
    let arg_expr = match argument_list.arguments.iter().nth(param_idx)? {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };

    // Try to extract a class name from the argument expression.
    if let Some(class_name) = extract_class_string_from_expr(arg_expr) {
        let fqn = ctx.written_names().resolve_class(&class_name);
        return Some(PhpType::named(atom(&fqn)));
    }

    // A variable holding a class name, read like any other variable: from
    // the walker's scope while the walker drives this narrowing, else
    // walked to at the argument itself, so a later reassignment of the
    // variable does not fold back into the assertion.
    if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
        let targets =
            crate::type_engine::variable::class_string_resolution::resolve_class_string_targets(
                bytes_to_str(dv.name),
                arg_expr.span().start.offset,
                ctx,
            );
        if let Some(first) = targets.first() {
            return Some(PhpType::named(first.fqn()));
        }
    }

    None
}

/// Unwrap parentheses and a single `!` prefix from a condition,
/// returning `(inner_expr, negated)`.
pub(in crate::type_engine) fn unwrap_condition_negation<'b>(
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

/// Cancel pairs of `!` off a condition, returning the equivalent
/// expression the AST already holds.
///
/// `!(!$user)` is `$user` and `!(!(!$user))` is `!$user`, so folding the
/// chain leaves each spelling in the polarity its extractor recognises.
/// Without it the outer `!` is a node no guard extractor matches, and a
/// condition that says exactly what `if ($user)` says narrows nothing.
/// Blade's `@unless (!$user)` compiles to the doubly negated form, so this
/// is a shape people write without meaning to.
///
/// Parentheses are transparent, since they change no meaning here.
pub(in crate::type_engine) fn fold_negation_pairs<'b>(
    expr: &'b Expression<'b>,
) -> &'b Expression<'b> {
    /// The operand of `expr` when it is a logical `!`.
    fn not_operand<'b>(expr: &'b Expression<'b>) -> Option<&'b Expression<'b>> {
        match expr {
            Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
                Some(crate::parser::unwrap_parens(prefix.operand))
            }
            _ => None,
        }
    }

    let mut current = crate::parser::unwrap_parens(expr);
    while let Some(once) = not_operand(current) {
        match not_operand(once) {
            Some(twice) => current = twice,
            None => break,
        }
    }
    current
}

/// The subject key an assertion tag names at the call site.
///
/// Most tags name a parameter (`@phpstan-assert Foo $value`), so the
/// subject is whichever argument was bound to it — that is
/// [`find_assertion_arg_variable`]. A tag can instead reach through a
/// member: `@phpstan-assert bool $this->resolved` promises something
/// about the object the call was made on, and `@phpstan-assert Foo
/// $value->inner` about a path below an argument. Both are written from
/// inside the callee, so the leading variable is rewritten to whatever
/// stands in its place at the call site and the rest of the path is
/// carried over unchanged.
///
/// Returns `None` when the leading variable has no counterpart here (a
/// `$this->…` tag on a static call, a receiver that is not a plain
/// variable, an argument that was not written).
pub(in crate::type_engine) fn assertion_subject_key(
    param_name: &str,
    info: &CallAssertionInfo<'_>,
) -> Option<String> {
    let Some(split) = param_name.find(['-', '[', ':']) else {
        return find_assertion_arg_variable(info.argument_list, param_name, &info.parameters);
    };
    let (root, rest) = param_name.split_at(split);
    let base = match root {
        "$this" => info.receiver_key.clone()?,
        param => find_assertion_arg_variable(info.argument_list, param, &info.parameters)?,
    };
    Some(format!("{base}{rest}"))
}

/// Given a function's argument list and a parameter name (with `$`
/// prefix), find the subject key passed at that parameter's position.
///
/// Returns the subject key for a direct variable (`$var`), a property
/// path (`$arg->value`), or an array access (`$stmts["0"]`) so that
/// assertion narrowing applies to non-variable subjects, not just plain
/// variables.
pub(in crate::type_engine) fn find_assertion_arg_variable(
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

/// Every subject key a tag on `param_name` names at the call site.
///
/// A variadic parameter collects every positional argument from its
/// position on, so an assertion about it is an assertion about each of
/// them. Anything else names at most the one argument
/// [`find_assertion_arg_variable`] finds.
pub(in crate::type_engine) fn find_assertion_arg_variables(
    argument_list: &ArgumentList<'_>,
    param_name: &str,
    parameters: &[crate::types::ParameterInfo],
) -> Vec<String> {
    let Some(param_idx) = parameters.iter().position(|p| p.name == param_name) else {
        return Vec::new();
    };
    if !parameters[param_idx].is_variadic {
        return find_assertion_arg_variable(argument_list, param_name, parameters)
            .into_iter()
            .collect();
    }
    // An unpacked `...$rest` hands over a whole array, not one value the
    // tag could speak for.
    argument_list
        .arguments
        .iter()
        .skip(param_idx)
        .filter_map(|arg| match arg {
            Argument::Positional(pos) if pos.ellipsis.is_none() => expr_to_subject_key(pos.value),
            _ => None,
        })
        .collect()
}
