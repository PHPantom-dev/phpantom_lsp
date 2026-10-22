//! Plain function call return-type resolution: a named function's
//! conditional, templated, or declared return type, the builtins whose
//! arguments decide their result, and invoking a callable value (`$fn()`,
//! `($expr)()`, `Foo::method(...)()`).

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ResolvedType;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast;
use crate::type_engine::variable::resolution::build_var_resolver_from_ctx;

use super::calls::{
    build_function_template_subs, extract_closure_or_arrow_return_type, resolve_return_type,
    substitute_function_templates, walker_arg_types,
};
use super::laravel_calls::laravel_helper_call;
use super::method_calls::resolve_rhs_method_call_inner;
use super::static_calls::{StaticCallParts, resolve_rhs_static_call};
use super::{resolve_rhs_expression, resolve_var_types, resolved_type_with_lookup};

/// Resolve a plain function call: `someFunc()`, array functions, variable
/// invocations (`$fn()`), and conditional return types.
pub(super) fn resolve_rhs_function_call<'b>(
    func_call: &'b FunctionCall<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let argument_list = &func_call.argument_list;

    if let Expression::PartialApplication(pa) = func_call.function
        && let Some(resolved) = first_class_callable_invocation(pa, argument_list, ctx)
    {
        return resolved;
    }

    if let Expression::Identifier(ident) = func_call.function {
        let name = bytes_to_str(ident.value());
        // Byte offset of the function-name identifier, so the loader can
        // consult mago-names' per-offset resolution.  This is what lets a
        // call resolve to a function declared in a *different* `namespace`
        // block of the same file (the file-level namespace guess would miss).
        let name_offset = func_call.function.span().start.offset;
        if let Some(resolved) = laravel_helper_call(name, argument_list, ctx)
            .or_else(|| builtin_function_call(name, argument_list, ctx))
            .or_else(|| named_function_call(name, name_offset, argument_list, ctx))
        {
            return resolved;
        }
    }

    callable_invocation(func_call.function, expr, ctx)
}

/// What calling the callable value `callee` returns, whatever form it is
/// written in: `$fn`, `($fn)`, `$this->handler`, a closure literal, or a
/// first-class callable such as `trim(...)`.
///
/// `at` is the expression making the call, above which an `@var` naming
/// the callable's type is looked for.  A call (`$fn()`) and a pipe
/// (`$x |> $fn`) both read their callee through here.
pub(super) fn callable_invocation<'b>(
    callee: &'b Expression<'b>,
    at: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // `($fn)()` calls the same value `$fn()` does.
    let callee = match callee {
        Expression::Parenthesized(p) => p.expression,
        other => other,
    };
    if let Expression::Variable(Variable::Direct(dv)) = callee {
        return variable_invocation(bytes_to_str(dv.name), at, ctx);
    }
    expression_invocation(callee, ctx)
}

/// What invoking a first-class callable on the spot returns:
/// `Foo::method(...)()`, `$obj->method(...)()` and `strlen(...)()` each
/// return what the call they wrap does.
///
/// `None` when the wrapped function cannot be found, leaving the caller to
/// read the callable's own type.
fn first_class_callable_invocation<'b>(
    pa: &'b PartialApplication<'b>,
    argument_list: &'b ArgumentList<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    match pa {
        PartialApplication::StaticMethod(sma) => Some(resolve_rhs_static_call(
            StaticCallParts {
                class: sma.class,
                method: &sma.method,
                argument_list: Some(argument_list),
            },
            ctx,
        )),
        PartialApplication::Method(ma) => Some(resolve_rhs_method_call_inner(
            ma.object,
            &ma.method,
            argument_list,
            ctx,
        )),
        PartialApplication::Function(fa) => Some(resolve_return_type(
            first_class_function_return_type(fa, ctx)?,
            ctx,
        )),
    }
}

/// The declared return type of the function a `name(...)` first-class
/// callable names, looked up where the name is written so a namespaced
/// call finds the function PHP would.
///
/// `None` when the callable wraps a value (`$fn(...)`) rather than naming
/// a function, or the function is not found or declares no return type.
pub(super) fn first_class_function_return_type(
    fa: &FunctionPartialApplication<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let Expression::Identifier(ident) = fa.function else {
        return None;
    };
    let function_loader = ctx.function_loader()?;
    function_loader(bytes_to_str(ident.value()), ident.span().start.offset)?.return_type
}

/// What a builtin returns when its arguments decide more than its stub
/// declares: a string function folded over literal arguments or refined by
/// them, or an array function's element or iterable type read from the
/// array it is handed.
fn builtin_function_call(
    name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    // ── String builtins ──────────────────────────────
    // The stub declares the widest string the function can return; with
    // literal arguments the call has one answer, and a caller checking
    // it against a literal union needs that answer rather than `string`.
    // Short of that, the arguments can still decide a refinement
    // (`lowercase-string`, `non-empty-string`).
    if let Some(folded) =
        crate::type_engine::variable::raw_type_inference::resolve_string_func_return_type(
            name,
            argument_list,
            ctx,
        )
    {
        return Some(vec![resolved_type_with_lookup(
            folded,
            ctx.file.all_classes,
            ctx.file.class_loader,
        )]);
    }

    // ── Known array functions ────────────────────────
    // For element-extracting functions (array_pop, etc.) resolve to the
    // element type.  An element that is not class-like (`array_pop` on a
    // `list<list<int>>` yields `list<int>`) is still the right answer;
    // falling through to the raw-type branch below would hand back the
    // container type unchanged.
    if let Some(element_type) =
        crate::type_engine::variable::raw_type_inference::resolve_array_func_element_type(
            name,
            argument_list,
            ctx,
        )
    {
        return Some(resolve_return_type(element_type, ctx));
    }

    // For type-preserving functions (array_filter, array_values, etc.)
    // the output has the same iterable type as the input array.
    // Return the full type string (e.g. `list<User>`) so that
    // downstream consumers (foreach, array access, hover) see the
    // element type without needing the raw-type pipeline's fallback.
    let raw_type = crate::type_engine::variable::raw_type_inference::resolve_array_func_raw_type(
        name,
        argument_list,
        ctx,
    )?;
    Some(resolve_return_type(raw_type, ctx))
}

/// What a call to the function `name` returns: the branch its conditional
/// return type or its arguments decide, its return type with function-level
/// `@template` parameters bound from the arguments, or its declared return
/// type.
///
/// `None` when no such function is loaded or it declares no return type.
fn named_function_call<'b>(
    name: &str,
    name_offset: u32,
    argument_list: &'b ArgumentList<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let function_loader = ctx.function_loader()?;
    let func_info = function_loader(name, name_offset)?;
    let current_class_fqn = ctx.current_class.fqn();
    let current_class_name: &str = &current_class_fqn;

    // A return type the call site decides (a conditional keyed on an
    // argument, or a builtin whose arguments decide its result) is
    // authoritative, so it is tried before the declared type.
    if func_info.conditional_return.is_some()
        || crate::type_engine::type_ops::call_site_returns::has_call_site_return(name)
    {
        let var_resolver = build_var_resolver_from_ctx(ctx);
        let rctx = ctx.as_resolution_ctx();
        // `is <Type>` conditions on an argument that isn't a literal
        // (`preg_replace($p, $r, $subject)`) are decided by the
        // argument's resolved type, so the branch a call takes matches
        // what it was actually handed.
        let arg_ty_resolver = |t: &str| Backend::resolve_arg_text_to_type(t, &rctx);
        let arg_texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
        let text_args = arg_texts.join(", ");
        let resolved_type = func_info.conditional_return.as_ref().and_then(|cond| {
            let tpl = crate::type_engine::type_ops::conditional::TemplateContext {
                defaults: None,
                params: &func_info.template_params,
                bindings: &func_info.template_bindings,
                arg_type_resolver: Some(&arg_ty_resolver),
                this_type: None,
            };
            crate::type_engine::conditional_resolution::resolve_conditional_with_text_args_and_defaults(
                cond,
                &func_info.parameters,
                &text_args,
                Some(&var_resolver),
                crate::type_engine::conditional_resolution::ConditionalClassContext {
                    calling: Some(current_class_name),
                    declaring: None,
                    written_class: Some(&|name: &str| ctx.written_names().resolve_class(name)),
                },
                ctx.file.class_loader,
                &tpl,
            )
        })
        // A builtin whose arguments decide its result (`json_encode(…,
        // JSON_THROW_ON_ERROR)` never returning `false`, `filter_var()`
        // returning what its filter produces) is decided the same way:
        // at the call site. The arguments are read through the walker
        // first, which types an inline options array as its shape where
        // the text resolver only sees `array`.
        .or_else(|| {
            let walker_types = walker_arg_types(argument_list, ctx);
            let arg_ty_resolver = |t: &str| walker_types(t).or_else(|| arg_ty_resolver(t));
            crate::type_engine::type_ops::call_site_returns::call_site_return_type(
                name,
                &func_info.parameters,
                &text_args,
                func_info.return_type.as_ref()?,
                Some(&arg_ty_resolver),
            )
        });
        if let Some(ty) = resolved_type {
            // The winning branch can name a function-level `@template`
            // (`tap()` returns `TValue`), which only the call-site
            // arguments fill in.
            let walker_types = walker_arg_types(argument_list, ctx);
            let ty = substitute_function_templates(
                &func_info,
                ty,
                &arg_texts,
                Some(&walker_types),
                &rctx,
            );
            return Some(resolve_return_type(ty, ctx));
        }
    }

    // ── Function-level @template substitution ────────────
    // When the function has template params and bindings,
    // infer concrete types from the arguments and apply
    // substitution to the return type before resolving.
    if !func_info.template_params.is_empty()
        && let Some(ref ret) = func_info.return_type
    {
        let arg_texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
        let rctx = ctx.as_resolution_ctx();
        let walker_types = walker_arg_types(argument_list, ctx);
        let subs = build_function_template_subs(&func_info, &arg_texts, Some(&walker_types), &rctx);
        if !subs.is_empty() {
            let substituted = ret.substitute(&subs);
            let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &substituted,
                current_class_name,
                ctx.file.all_classes,
                ctx.file.class_loader,
            );
            if !resolved.is_empty() {
                return Some(ResolvedType::from_classes_with_hint(resolved, substituted));
            }
            // The substituted type didn't resolve to any classes
            // (e.g. `mixed|null`, `int|null`, `array-key|null`).
            // Return it as a type-string-only entry so that
            // downstream consumers see the substituted type
            // instead of the raw template name.
            return Some(vec![ResolvedType::from_type_string(substituted)]);
        }
    }

    Some(resolve_return_type(func_info.return_type?, ctx))
}

/// What `$fn()` returns: read from the variable's annotation, the callable
/// type it holds, an array callable, or its class's `__invoke()`.
fn variable_invocation(
    var_name: &str,
    _expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // 1. Resolve the variable's own type.  Closures, arrow functions,
    //    and first-class callables are all inferred by
    //    `resolve_rhs_expression` as a `TypeKind::Callable` (see
    //    `infer_closure_literal_type`), so `$fn`'s embedded return
    //    type covers `$fn = function(): T {}`, `$fn = fn(): T => …`,
    //    and `$fn = strlen(...)` / `$fn = $obj->method(...)` alike.
    let var_types = resolve_var_types(var_name, ctx, ctx.cursor_offset);
    if let Some(resolved) = callable_typed_return(&var_types, ctx) {
        return resolved;
    }

    // 2. An array callable, `[$obj, 'method']`, calls the method.
    for rt in &var_types {
        let resolved = array_callable_return(&rt.type_string, ctx);
        if !resolved.is_empty() {
            return resolved;
        }
    }

    // 3. An object with an `__invoke()` method returns what that does.
    invoke_method_return(&var_types, ctx)
}

/// What `($expr)()` returns when the callee is an arbitrary expression
/// (`($this->foo)()`, `(getFactory())()`, a closure literal invoked on the
/// spot): the literal's own return type, the callable type the expression
/// resolves to, or its class's `__invoke()`.
fn expression_invocation<'b>(
    callee_expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // ── Directly invoked closure / arrow function ────
    // `(fn (): Foo => …)()` or `(function (): Foo { … })()`
    // Extract the return type from the literal instead of going
    // through `__invoke()` on the generic `Closure` stub.
    if let Some(parsed_ret_type) = extract_closure_or_arrow_return_type(callee_expr, ctx) {
        let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &parsed_ret_type,
            &ctx.current_class.fqn(),
            ctx.file.all_classes,
            ctx.file.class_loader,
        );
        if !resolved.is_empty() {
            return ResolvedType::from_classes_with_hint(resolved, parsed_ret_type);
        }
    }

    let callee_results = resolve_rhs_expression(callee_expr, ctx);
    // A callable-typed value carries its return type in the type
    // string itself (`callable(): Scope`, `Closure(): Item`), which is
    // how a property or a method result annotated that way arrives
    // here.  Read it the same way the `$fn()` path does before falling
    // back to `__invoke()`.
    if let Some(resolved) = callable_typed_return(&callee_results, ctx) {
        return resolved;
    }
    invoke_method_return(&callee_results, ctx)
}

/// What calling a value of one of `types` returns, read from the callable
/// signature its type carries (`callable(): Scope`, `Closure(): Item`).
fn callable_typed_return(
    types: &[ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if let Some(ret_type) = joined_callable_return(types) {
        let resolved = callable_return_resolution(&ret_type, ctx);
        if !resolved.is_empty() {
            return Some(resolved);
        }
    }
    types.iter().find_map(|rt| {
        let resolved = callable_return_resolution(rt.type_string.callable_return_type()?, ctx);
        (!resolved.is_empty()).then_some(resolved)
    })
}

/// What calling an object of one of `types` returns through its class's
/// `__invoke()` method.
fn invoke_method_return(types: &[ResolvedType], ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    for owner in types.iter().filter_map(|rt| rt.class_info.as_ref()) {
        if let Some(invoke) = owner.get_method("__invoke")
            && let Some(ref ret) = invoke.return_type
        {
            // A return type that names no class (e.g. `Item[]`) still
            // comes back as a type-string-only entry, so callers like
            // foreach resolution can extract its element type.
            let resolved = callable_return_resolution(ret, ctx);
            if !resolved.is_empty() {
                return resolved;
            }
        }
    }
    vec![]
}

/// What calling a value of one of `types` returns, when every alternative
/// is callable by its type alone, or `None` when some alternative is not.
///
/// Each alternative contributes its own return type, so the answer is
/// their join: `(Route&callable)|(callable(): Route)` can return anything,
/// because the callable `Route` subclass declares no return type.  A
/// callable spelled without a signature returns `mixed`.  An alternative
/// that is callable only through a class's `__invoke()` is left to the
/// callers' own lookup.
fn joined_callable_return(types: &[ResolvedType]) -> Option<PhpType> {
    fn member_return(member: &PhpType) -> Option<PhpType> {
        match member.kind() {
            TypeKind::Callable(c) => Some(c.return_type.clone().unwrap_or_else(PhpType::mixed)),
            TypeKind::Named(_) if member.is_callable() => Some(PhpType::mixed()),
            TypeKind::Intersection(parts) => parts.iter().find_map(member_return),
            _ => None,
        }
    }
    let mut returns: Vec<PhpType> = Vec::new();
    for rt in types {
        for member in rt.type_string.union_members() {
            if member.is_null() {
                continue;
            }
            returns.push(member_return(member)?);
        }
    }
    // A single alternative already has its answer in the per-entry paths,
    // which also know how to resolve the `Closure` stub's `__invoke()`.
    if returns.len() < 2 {
        return None;
    }
    Some(PhpType::join_runtime_value_types(returns))
}

/// What calling a value whose callable type returns `ret_type` produces.
fn callable_return_resolution(ret_type: &PhpType, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    if ret_type.is_empty() {
        return vec![];
    }
    resolve_return_type(ret_type.clone(), ctx)
}

/// What `(closure)->call($obj)` returns: whatever the closure literal does,
/// run with `$this` bound to `$obj`.
pub(super) fn closure_call_return(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let closure = crate::parser::unwrap_parens(object);
    if let Some(declared) = extract_closure_or_arrow_return_type(closure, ctx) {
        let resolved = callable_return_resolution(&declared, ctx);
        return (!resolved.is_empty()).then_some(resolved);
    }
    let Expression::ArrowFunction(arrow) = closure else {
        return None;
    };
    let mut body_ctx = ctx.clone();
    if let Some(new_this) = argument_list.arguments.first() {
        let bound = resolve_rhs_expression(new_this.value(), ctx);
        if !bound.is_empty() {
            body_ctx
                .match_arm_narrowing
                .insert("$this".to_string(), bound);
        }
    }
    let resolved = resolve_rhs_expression(arrow.expression, &body_ctx);
    (!resolved.is_empty()).then_some(resolved)
}

/// What calling the array callable `callable` (`[$obj, 'method']`) returns.
///
/// Only an object in the first slot is read: `['Foo', 'bar']` calls `bar`
/// statically, which is a different call with rules of its own.
fn array_callable_return(callable: &PhpType, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    let Some([receiver, method]) = callable.shape_entries() else {
        return vec![];
    };
    let positional = |entry: &crate::php_type::ShapeEntry, index: &str| {
        !entry.optional && entry.key.as_deref().is_none_or(|k| k == index)
    };
    if !positional(receiver, "0") || !positional(method, "1") {
        return vec![];
    }
    let TypeKind::Literal(literal) = method.value_type.kind() else {
        return vec![];
    };
    let Some(method_name) = literal.string_content() else {
        return vec![];
    };
    if !receiver.value_type.is_object_like() {
        return vec![];
    }
    let owners = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &receiver.value_type,
        &ctx.current_class.fqn(),
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    for owner in &owners {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            owner,
            ctx.file.class_loader,
            ctx.resolved_class_cache,
        );
        if let Some(ret) = merged
            .get_method(&method_name)
            .and_then(|m| m.return_type.as_ref())
        {
            let ret = ret.replace_self_bound(&owner.fqn(), None);
            return callable_return_resolution(&ret, ctx);
        }
    }
    vec![]
}
