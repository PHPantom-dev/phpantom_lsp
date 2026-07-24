/// Function/method/static call return-type resolution: resolves
/// function calls, method calls, and static calls to their return
/// types, including template substitution for `@template` parameters
/// and conditional return type evaluation.
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

use crate::completion::call_resolution::MethodReturnCtx;
use crate::completion::conditional_resolution::resolve_conditional_with_args;
use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::completion::variable::resolution::build_var_resolver_from_ctx;

use super::array_access::{class_string_inner_binding, insert_or_union};
use super::instantiation::{
    TemplateBindingMode, classify_template_binding, extract_generic_arg_from_ancestor,
};
use super::{
    extract_closure_or_arrow_return_type, infer_if_this_is_subs, resolve_rhs_expression,
    resolve_var_types, resolved_type_with_lookup,
};

/// Build a template substitution map for a function-level `@template` call.
///
/// Uses the function's `template_bindings` to match template parameters to
/// their concrete types inferred from the call-site arguments.  Handles:
///   - Direct type: `@param T $bar` + `func(new Baz())` → `T = Baz`
///   - Array type: `@param T[] $items` + `func([new X()])` → `T = X`
///   - Generic wrapper: `@param array<TKey, TValue> $v` + `func($users)` →
///     positional resolution through the wrapper's generic arguments.
pub(crate) fn build_function_template_subs(
    func_info: &crate::types::FunctionInfo,
    arg_texts: &[String],
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut subs = HashMap::new();

    // Bind the raw source-order argument texts to parameters by PHP's rules
    // so a named argument (`id: Foo::class`) is routed to the parameter it
    // targets rather than its ordinal slot, and its `name:` prefix is
    // stripped off the value.
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
    let bound = crate::call_args::bind_text_args_to_params(&func_info.parameters, &arg_refs);

    for (tpl_name, param_name) in &func_info.template_bindings {
        let param_idx = match func_info
            .parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
        {
            Some(idx) => idx,
            None => continue,
        };

        let provided_arg = bound.get(param_idx).and_then(|o| o.as_deref());

        // Determine the binding mode by inspecting the parameter's
        // docblock type hint.  The type hint tells us how the template
        // param is embedded in the `@param` annotation.
        let param_hint = func_info
            .parameters
            .get(param_idx)
            .and_then(|p| p.type_hint.as_ref());
        let binding_mode = classify_template_binding(tpl_name, param_hint);

        // Fall back to the parameter's default value only for binding
        // modes where the default is meaningful (class-string<T> with
        // a `Foo::class` default, or direct bindings with `::class`).
        let default_value = func_info
            .parameters
            .get(param_idx)
            .and_then(|p| p.default_value.as_deref());
        let arg_text: &str = match provided_arg {
            Some(text) => text,
            None => match &binding_mode {
                TemplateBindingMode::ClassStringInner => match default_value {
                    Some(d) => d,
                    None => continue,
                },
                TemplateBindingMode::Direct => match default_value {
                    Some(d) if d.ends_with("::class") => d,
                    _ => continue,
                },
                _ => continue,
            },
        };

        match binding_mode {
            TemplateBindingMode::Direct => {
                if let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
                    insert_or_union(&mut subs, tpl_name.to_string(), resolved_type);
                }
            }
            TemplateBindingMode::CallableReturnType => {
                // `@param callable(...): T $cb` — infer the closure's return
                // type from its annotation, generator yields, or (for
                // unannotated closures) its resolved body expression.
                if let Some(ret_type) = Backend::infer_closure_return_type(arg_text, rctx) {
                    insert_or_union(&mut subs, tpl_name.to_string(), ret_type);
                }
            }
            TemplateBindingMode::CallableParamType(position) => {
                // `@param Closure(T): void $cb` — extract the closure's
                // parameter type annotation at the given position.
                if let Some(param_type) =
                    crate::completion::source::helpers::extract_closure_param_type_from_text(
                        arg_text, position,
                    )
                {
                    insert_or_union(&mut subs, tpl_name.to_string(), param_type);
                }
            }
            TemplateBindingMode::ArrayElement => {
                // `@param T[] $items` — resolve individual array elements.
                // Empty array `[]` → element type is `never`.
                if arg_text.starts_with('[') && arg_text.ends_with(']') {
                    let inner = arg_text[1..arg_text.len() - 1].trim();
                    if inner.is_empty() {
                        // Empty array `[]` → element type is `never`.
                        subs.insert(tpl_name.to_string(), PhpType::never());
                    } else {
                        let first_elem =
                            crate::completion::conditional_resolution::split_text_args(inner);
                        if let Some(elem) = first_elem.first()
                            && let Some(resolved_type) =
                                Backend::resolve_arg_text_to_type(elem.trim(), rctx)
                        {
                            subs.insert(tpl_name.to_string(), resolved_type);
                        }
                    }
                } else if let Some(resolved_type) =
                    Backend::resolve_arg_text_to_type(arg_text, rctx)
                        .or_else(|| resolve_arg_call_raw_type(arg_text, rctx))
                {
                    // Extract the element type from array-like types
                    // so we bind T to the element, not the whole array.
                    // The call-expression fallback covers arguments whose
                    // declared return type is an array (`getConfigs()`
                    // returning `array<string, Config>`) — those carry no
                    // class info, so the general resolver yields nothing.
                    if let Some(elem_type) = resolved_type.extract_value_type(false) {
                        insert_or_union(&mut subs, tpl_name.to_string(), elem_type.clone());
                    } else if !resolved_type.is_array_like() {
                        // The argument resolved to a genuine (non-array)
                        // type — bind it directly.  A bare array-like
                        // container whose element type can't be extracted
                        // is left unbound so `T` falls back to its bound
                        // (or `mixed`) rather than binding `T` to `array`.
                        insert_or_union(&mut subs, tpl_name.to_string(), resolved_type);
                    }
                }
            }
            TemplateBindingMode::ClassStringInner => {
                if let Some(binding) = class_string_inner_binding(arg_text, rctx) {
                    insert_or_union(&mut subs, tpl_name.to_string(), binding);
                }
            }
            TemplateBindingMode::GenericWrapper(ref wrapper_name, tpl_position) => {
                // When the argument is a closure and the param hint
                // union contains a Callable variant, try yield inference
                // before array-like or hierarchy extraction.
                if let Some(concrete) = Backend::try_closure_return_type_for_template(
                    arg_text,
                    tpl_name,
                    tpl_position,
                    param_hint,
                    rctx,
                ) {
                    insert_or_union(&mut subs, tpl_name.to_string(), concrete);
                    continue;
                }
                // For `@param array<TKey, TValue> $value`, resolve the
                // argument's raw iterable type — from a variable's
                // annotations/assignments (`$users` as `array<int, User>`)
                // or from a call expression's declared return type
                // (`$this->getUsers()` returning `array<int, User>`) —
                // and extract the positional generic argument.
                if is_array_like_wrapper(wrapper_name)
                    && let Some(resolved) = resolve_arg_iterable_raw_type(arg_text, rctx)
                    && let Some(concrete) = extract_array_type_at_position(&resolved, tpl_position)
                {
                    subs.insert(tpl_name.to_string(), concrete);
                    continue;
                }
                // Array literal argument for array-like wrappers:
                // `[1, 2, 3]` for `@param array<T>` → infer T from elements.
                if is_array_like_wrapper(wrapper_name)
                    && arg_text.starts_with('[')
                    && arg_text.ends_with(']')
                {
                    let inner = arg_text[1..arg_text.len() - 1].trim();
                    if inner.is_empty() {
                        // Empty array `[]` → element type is `never`.
                        subs.insert(tpl_name.to_string(), PhpType::never());
                        continue;
                    } else {
                        let elems =
                            crate::completion::conditional_resolution::split_text_args(inner);
                        // For `array<T>` (position 0 with 1 generic arg) or
                        // `array<K, V>` (position 1 = value), infer from
                        // element values.  For position 0 in a 2-arg generic
                        // (the key), infer from keys if available.
                        if let Some(elem) = elems.first()
                            && let Some(resolved_type) =
                                Backend::resolve_arg_text_to_type(elem.trim(), rctx)
                        {
                            subs.insert(tpl_name.to_string(), resolved_type);
                            continue;
                        }
                    }
                }
                // Special case: unwrap class-string<class-string<T>> to class-string<T>
                if wrapper_name == "class-string"
                    && tpl_position == 0
                    && let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx)
                {
                    if let Some(inner) = resolved_type.unwrap_class_string_inner() {
                        subs.insert(tpl_name.to_string(), inner.clone());
                    } else {
                        subs.insert(tpl_name.to_string(), resolved_type);
                    }
                }
                // ── Class generic wrapper resolution ────────────────
                // For `@param Container<TItem> $c` where the argument
                // is a subclass like `FooContainer extends Container<Foo>`,
                // resolve the argument type and walk its @extends chain
                // to find the wrapper class's generic arg at the right
                // position.
                if !is_array_like_wrapper(wrapper_name)
                    && wrapper_name != "class-string"
                    && let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx)
                    && let Some(concrete) = extract_generic_arg_from_ancestor(
                        &resolved_type,
                        wrapper_name,
                        tpl_position,
                        rctx,
                    )
                {
                    subs.insert(tpl_name.to_string(), concrete);
                    continue;
                }
                // When array-type extraction fails (e.g. bare `array`
                // property without generic annotation), do NOT fall back
                // to a Direct resolve — that would bind the template
                // param to the whole argument type instead of its
                // positional generic arg.  Leave it unbound so the
                // "fill in unbound" code below maps it to its declared
                // upper bound or `mixed`.
            }
        }
    }

    // ── Fill in unbound function-level template params ──────
    // Any template parameter that was not bound from call-site
    // arguments is replaced with its declared upper bound
    // (`@template T of Foo` → `Foo`) or `mixed`.  This follows
    // PHPStan's `resolveToBounds()` semantics and prevents raw
    // template names like `TReduceReturnType` from leaking into
    // parameter and return types.
    for tpl_name in &func_info.template_params {
        let tpl_key = tpl_name.to_string();
        subs.entry(tpl_key).or_insert_with(|| {
            func_info
                .template_param_bounds
                .get(tpl_name)
                .cloned()
                .unwrap_or_else(PhpType::mixed)
        });
    }

    subs
}

/// Resolve a variable argument to its raw type string.
///
/// For `$pens` with `/** @var Pen[] $pens */`, returns `Some("Pen[]")`.
/// For `$users` with `/** @var array<int, User> $users */`, returns
/// `Some("array<int, User>")`.
///
/// Tries docblock annotations first, then falls back to AST-based
/// raw type inference.
pub(super) fn resolve_arg_variable_raw_type(
    arg_text: &str,
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let var_name = arg_text.trim();
    if !var_name.starts_with('$') {
        return None;
    }

    // ── Property chain: `$this->items`, `$obj->prop` ────────────
    // When the argument is a property access chain, resolve the base
    // object's type and look up the property's type hint.  This is
    // needed for template substitution in calls like
    // `array_any($this->items, fn($item) => …)` where `$this->items`
    // is `array<int, PurchaseFileProduct>` after generic substitution.
    if let Some(arrow_pos) = var_name.find("->") {
        let base = &var_name[..arrow_pos];
        let prop = &var_name[arrow_pos + 2..];
        // Only handle simple single-level property access for now.
        if !prop.is_empty() && !prop.contains("->") && !prop.contains('(') {
            let base_classes = ResolvedType::into_arced_classes(
                crate::completion::resolver::resolve_target_classes(
                    base,
                    crate::types::AccessKind::Arrow,
                    rctx,
                ),
            );
            for cls in &base_classes {
                if let Some(hint) =
                    crate::inheritance::resolve_property_type_hint(cls, prop, rctx.class_loader)
                {
                    return Some(hint);
                }
            }
        }
    }

    // 1. Try docblock annotation (@var).
    if let Some(raw) = crate::docblock::find_iterable_raw_type_in_source(
        rctx.content,
        rctx.cursor_offset as usize,
        var_name,
    )
    .map(|t| crate::util::resolve_php_type_names(&t, rctx.class_loader))
    {
        return Some(raw);
    }

    // 2. When the diagnostic scope cache is active (and not still being
    //    built), read the variable's type from the pre-computed forward-
    //    walked scope snapshots.  This avoids hitting the backward
    //    scanner during diagnostic collection.
    if crate::completion::variable::forward_walk::is_diagnostic_scope_active()
        && !crate::completion::variable::forward_walk::is_building_scopes()
    {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        if let Some(types) = crate::completion::variable::forward_walk::lookup_diagnostic_scope(
            &prefixed,
            rctx.cursor_offset,
        ) {
            return Some(ResolvedType::types_joined(&types));
        }
    }

    // 3. When a scope_var_resolver is available (forward walker is
    //    active on either diagnostic or completion path), read from
    //    the in-progress ScopeState.  If the variable isn't there,
    //    it hasn't been assigned yet — return None rather than
    //    falling through to resolve_variable_types which would
    //    re-enter the forward walker and cause stack overflow.
    if let Some(resolver) = rctx.scope_var_resolver {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        let from_scope = resolver(&prefixed);
        if from_scope.is_empty() {
            return None;
        }
        return Some(ResolvedType::types_joined(&from_scope));
    }

    // 4. During the build phase, the forward walker is the authority.
    //    If the variable isn't in the scope cache, don't fall through
    //    to the backward scanner — return None so the caller treats
    //    it as unresolved.
    if crate::completion::variable::forward_walk::is_building_scopes() {
        return None;
    }

    // 5. Fall back to unified variable resolution pipeline (backward
    //    scanner).  This path is only reached for interactive features
    //    (hover, completion, goto-def) where no scope cache is active
    //    and no scope_var_resolver was provided.
    //
    // Guard: resolve_variable_types is designed for bare `$variable`
    // names.  Complex expressions (array access like `$arr['key']`,
    // comparisons like `$x === 'foo'`, boolean chains, null coalescing)
    // are not variable names and will never match a scope entry.
    // Skip them to avoid wasted backward scans and fallthrough noise.
    if var_name.contains("->")
        || var_name.contains("::")
        || var_name.contains('[')
        || var_name.contains("===")
        || var_name.contains("&&")
        || var_name.contains("??")
        || var_name.contains("||")
    {
        return None;
    }

    let default_class = crate::types::ClassInfo::default();
    let current_class = rctx.current_class.unwrap_or(&default_class);
    let resolved = crate::completion::variable::resolution::resolve_variable_types(
        var_name,
        current_class,
        rctx.all_classes,
        rctx.content,
        rctx.cursor_offset,
        rctx.class_loader,
        Loaders::with_function(rctx.function_loader),
    );
    if resolved.is_empty() {
        None
    } else {
        Some(ResolvedType::types_joined(&resolved))
    }
}

/// Resolve a call-expression argument (`$obj->method()`, `self::method()`,
/// `helper()`) to its declared return type, preserving generic arguments
/// that don't resolve to loadable classes (e.g. `array<string, Config>`).
///
/// Routes through the shared call-resolution pipeline
/// (`resolve_call_return_types_expr_with_hint`) so class-level and
/// method-level template substitutions apply to the returned type.
/// Returns `None` when the text is not a call expression or the callee
/// has no declared return type.
pub(super) fn resolve_arg_call_raw_type(
    arg_text: &str,
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    if !trimmed.ends_with(')') {
        return None;
    }
    // Closure/arrow-function literals also end with `)` but are not
    // call expressions — their types are handled by the callable
    // binding modes, not here.
    if crate::completion::source::helpers::is_closure_like_text(trimmed) {
        return None;
    }
    let expr = crate::subject_expr::SubjectExpr::parse(trimmed);
    let crate::subject_expr::SubjectExpr::CallExpr { callee, args_text } = expr else {
        return None;
    };
    let mut hint: Option<PhpType> = None;
    Backend::resolve_call_return_types_expr_with_hint(&callee, &args_text, rctx, Some(&mut hint));
    hint
}

/// Resolve an argument's raw iterable type for positional generic
/// extraction, regardless of the argument's syntax shape.
///
/// Variables and property chains resolve through
/// [`resolve_arg_variable_raw_type`] (docblock annotations, forward-walk
/// scope, assignment scanning); call expressions resolve through the
/// shared call return-type pipeline via [`resolve_arg_call_raw_type`].
pub(super) fn resolve_arg_iterable_raw_type(
    arg_text: &str,
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    resolve_arg_variable_raw_type(arg_text, rctx)
        .or_else(|| resolve_arg_call_raw_type(arg_text, rctx))
}

/// Extract the concrete type at `position` from an array type string.
///
/// For array types with two generic parameters (key + value):
/// - `array<int, User>` at position 0 → `"int"`, position 1 → `"User"`
/// - `User[]` at position 0 → `"int"` (implicit key), position 1 → `"User"`
/// - `list<User>` at position 0 → `"int"`, position 1 → `"User"`
///
/// For single-param forms:
/// - `array<User>` at position 0 → `"User"`
pub(super) fn extract_array_type_at_position(ty: &PhpType, position: usize) -> Option<PhpType> {
    match position {
        0 => ty.extract_key_type(false).cloned(),
        1 => ty.extract_value_type(false).cloned(),
        _ => None,
    }
}

/// Whether a wrapper type name should be treated as array-like for
/// positional generic argument extraction.
///
/// When `@param Wrapper<TKey, TValue> $value` binds a template param
/// via `GenericWrapper`, and the wrapper is an array-like type, we can
/// resolve the argument variable's raw type (e.g. `User[]`) and extract
/// the positional generic component (key at 0, value at 1).
///
/// This covers `array`, `iterable`, `list`, and common Laravel/PHPStan
/// collection interfaces whose generic args follow `<TKey, TValue>`.
pub(crate) fn is_array_like_wrapper(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "array" | "list" | "non-empty-array" | "non-empty-list" | "iterable"
    ) || crate::util::short_name(name).eq_ignore_ascii_case("arrayable")
}

/// Resolve function, method, and static method calls to their return
/// types.
pub(super) fn resolve_rhs_call<'b>(
    call: &'b Call<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    match call {
        Call::Function(func_call) => resolve_rhs_function_call(func_call, expr, ctx),
        Call::Method(method_call) => resolve_rhs_method_call_inner(
            method_call.object,
            &method_call.method,
            &method_call.argument_list,
            ctx,
        ),
        Call::NullSafeMethod(method_call) => resolve_rhs_method_call_inner(
            method_call.object,
            &method_call.method,
            &method_call.argument_list,
            ctx,
        ),
        Call::StaticMethod(static_call) => resolve_rhs_static_call(static_call, ctx),
    }
}

pub(crate) fn infer_closure_literal_type(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    let explicit_or_yield = {
        let span = expr.span();
        let start = (span.start.offset as usize).min(ctx.content.len());
        let end = (span.end.offset as usize).min(ctx.content.len());
        ctx.content.get(start..end).and_then(|text| {
            crate::completion::source::helpers::extract_closure_return_type_from_text(text).or_else(
                || {
                    crate::completion::source::helpers::infer_generator_type_from_closure_yields(
                        text,
                    )
                },
            )
        })
    };

    let inferred_return = explicit_or_yield.or_else(|| match expr {
        Expression::ArrowFunction(arrow) => {
            let resolved = resolve_rhs_expression(arrow.expression, ctx);
            if resolved.is_empty() {
                None
            } else {
                Some(ResolvedType::types_joined(&resolved))
            }
        }
        // First-class callable syntax: `strlen(...)`, `$this->method(...)`,
        // `ClassName::method(...)`.  Resolve the underlying function/method's
        // return type from the callable's own source text.
        Expression::PartialApplication(_) => {
            let span = expr.span();
            let start = (span.start.offset as usize).min(ctx.content.len());
            let end = (span.end.offset as usize).min(ctx.content.len());
            ctx.content.get(start..end).and_then(|text| {
                let rctx = ctx.as_resolution_ctx();
                crate::completion::source::helpers::resolve_first_class_callable_return_type(
                    text, &rctx,
                )
            })
        }
        _ => None,
    });

    if let Some(ret) = inferred_return {
        PhpType::Callable {
            kind: "Closure".to_string(),
            params: Vec::new(),
            return_type: Some(Box::new(ret)),
        }
    } else {
        PhpType::closure()
    }
}

/// Resolve a plain function call: `someFunc()`, array functions, variable
/// invocations (`$fn()`), and conditional return types.
pub(super) fn resolve_rhs_function_call<'b>(
    func_call: &'b FunctionCall<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class_name: &str = &ctx.current_class.name;
    let all_classes = ctx.all_classes;
    let content = ctx.content;
    let class_loader = ctx.class_loader;
    let function_loader = ctx.function_loader();

    // ── First-class callable invocation: `Foo::method(...)()` ───
    // When the callee is a partial application (first-class callable),
    // invoking it with `()` returns the underlying method's return
    // type.  Delegate to the matching call-resolution path.
    if let Expression::PartialApplication(pa) = func_call.function {
        use mago_syntax::cst::partial_application::PartialApplication;
        match pa {
            PartialApplication::StaticMethod(sma) => {
                // For first-class callable invocation through late-static-binding
                // targets (self::, static::, parent::), preserve `static` in the
                // return type rather than resolving to the concrete class name.
                let is_late_static = matches!(
                    sma.class,
                    Expression::Self_(_) | Expression::Static(_) | Expression::Parent(_)
                );
                if is_late_static {
                    // Look up the method's original return type to check if
                    // it contains static/self/$this before resolution replaces it.
                    let method_name = match sma.method {
                        ClassLikeMemberSelector::Identifier(ident) => {
                            bytes_to_str(ident.value).to_string()
                        }
                        _ => String::new(),
                    };
                    if !method_name.is_empty() {
                        // Check current class first, then walk parent chain.
                        let method_ret = ctx
                            .current_class
                            .get_method_ci(&method_name)
                            .and_then(|m| m.return_type.clone());
                        let method_ret = method_ret.or_else(|| {
                            // Walk parent chain to find the method.
                            let mut parent_name = ctx
                                .current_class
                                .parent_class
                                .as_ref()
                                .map(|a| a.to_string());
                            while let Some(ref p) = parent_name {
                                if let Some(cls) = (ctx.class_loader)(p) {
                                    if let Some(m) = cls.get_method_ci(&method_name) {
                                        return m.return_type.clone();
                                    }
                                    parent_name = cls.parent_class.as_ref().map(|a| a.to_string());
                                } else {
                                    break;
                                }
                            }
                            None
                        });
                        if let Some(ref ret) = method_ret
                            && ret.contains_self_ref()
                        {
                            return vec![ResolvedType::from_type_string(PhpType::static_())];
                        }
                    }
                }
                // Build a synthetic StaticMethodCall and resolve it.
                let synthetic = mago_syntax::cst::call::StaticMethodCall {
                    class: sma.class,
                    double_colon: sma.double_colon,
                    method: sma.method.clone(),
                    argument_list: func_call.argument_list.clone(),
                };
                return resolve_rhs_static_call(&synthetic, ctx);
            }
            PartialApplication::Method(ma) => {
                let receiver_is_this = matches!(
                    ma.object,
                    Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
                );
                if receiver_is_this {
                    // Look up the method's original return type to check if
                    // it contains static/self/$this.
                    let method_name = match ma.method {
                        ClassLikeMemberSelector::Identifier(ident) => {
                            bytes_to_str(ident.value).to_string()
                        }
                        _ => String::new(),
                    };
                    if !method_name.is_empty() {
                        let method_ret = ctx
                            .current_class
                            .get_method_ci(&method_name)
                            .and_then(|m| m.return_type.clone());
                        let method_ret = method_ret.or_else(|| {
                            let mut parent_name = ctx
                                .current_class
                                .parent_class
                                .as_ref()
                                .map(|a| a.to_string());
                            while let Some(ref p) = parent_name {
                                if let Some(cls) = (ctx.class_loader)(p) {
                                    if let Some(m) = cls.get_method_ci(&method_name) {
                                        return m.return_type.clone();
                                    }
                                    parent_name = cls.parent_class.as_ref().map(|a| a.to_string());
                                } else {
                                    break;
                                }
                            }
                            None
                        });
                        if let Some(ref ret) = method_ret
                            && ret.contains_self_ref()
                        {
                            return vec![ResolvedType::from_type_string(PhpType::static_())];
                        }
                    }
                }
                return resolve_rhs_method_call_inner(
                    ma.object,
                    &ma.method,
                    &func_call.argument_list,
                    ctx,
                );
            }
            PartialApplication::Function(fa) => {
                // `strlen(...)()` — resolve the inner function name.
                if let Expression::Identifier(ident) = fa.function {
                    let name = bytes_to_str(ident.value()).to_string();
                    let name_offset = ident.span().start.offset;
                    let function_loader = ctx.function_loader();
                    if let Some(fl) = function_loader
                        && let Some(func_info) = fl(&name, name_offset)
                        && let Some(ref ret) = func_info.return_type
                    {
                        let resolved =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                ret,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved.is_empty() {
                            return ResolvedType::from_classes_with_hint(resolved, ret.clone());
                        }
                        return vec![resolved_type_with_lookup(
                            ret.clone(),
                            &ctx.current_class.name,
                            ctx.all_classes,
                            ctx.class_loader,
                        )];
                    }
                }
            }
        }
    }

    let func_name = match func_call.function {
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        _ => None,
    };
    // Byte offset of the function-name identifier, so the loader can
    // consult mago-names' per-offset resolution.  This is what lets a
    // call resolve to a function declared in a *different* `namespace`
    // block of the same file (the file-level namespace guess would miss).
    let func_name_offset = func_call.function.span().start.offset;

    // ── Laravel container string binding ────────────────
    // `$var = app('blade.compiler')` / `$var = resolve('cache')` bind a
    // plain string to a concrete class via the framework's container
    // alias table. Mirrors the direct-call-subject interception in
    // call_resolution.rs so the binding survives being assigned to a
    // variable instead of being chained off the call directly.
    if let Some(ref name) = func_name {
        let normalized_func = name.trim_start_matches('\\');
        if matches!(normalized_func, "app" | "resolve") {
            let arg_texts =
                crate::completion::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &func_call.argument_list,
                    content,
                );
            if let Some(first_arg) = arg_texts.first()
                && let Some(alias) = crate::util::unescape_php_string_literal(first_arg.trim())
                && let Some(cls) = (ctx.class_loader)(&alias)
            {
                return ResolvedType::from_classes(vec![cls]);
            }
        }

        // ── now() / today() → configured Laravel date class ──
        // Laravel's `now()`/`today()` helpers are declared to return
        // `CarbonInterface`, but they instantiate the concrete class selected
        // by Laravel's date factory.
        // Resolving to the interface loses the concrete type and
        // produces spurious mismatches when the value flows into a
        // `DateTime`/`DateTimeImmutable` declaration.  Map both to the
        // concrete class.
        //
        // This is not strictly sound (the helpers' declared type is the
        // interface), but it mirrors Larastan's `NowAndTodayExtension`.
        // The Laravel/Carbon ecosystem is written against that model, so
        // real codebases assume the concrete type; matching it avoids a
        // flood of mismatches that only exist because the declared types
        // are looser than reality.
        if matches!(
            normalized_func,
            "now" | "today" | "Illuminate\\Support\\now" | "Illuminate\\Support\\today"
        ) && let Some(cls) =
            (ctx.class_loader)(crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)
        {
            return ResolvedType::from_classes(vec![cls]);
        }
    }

    // ── Known array functions ────────────────────────
    // For element-extracting functions (array_pop, etc.)
    // resolve to the element ClassInfo directly.
    if let Some(ref name) = func_name
        && let Some(element_type) =
            crate::completion::variable::raw_type_inference::resolve_array_func_element_type(
                name,
                &func_call.argument_list,
                ctx,
            )
    {
        let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            &element_type,
            current_class_name,
            all_classes,
            class_loader,
        );
        if !resolved.is_empty() {
            return ResolvedType::from_classes_with_hint(resolved, element_type);
        }
    }

    // For type-preserving functions (array_filter, array_values, etc.)
    // the output has the same iterable type as the input array.
    // Return the full type string (e.g. `list<User>`) so that
    // downstream consumers (foreach, array access, hover) see the
    // element type without needing the raw-type pipeline's fallback.
    if let Some(ref name) = func_name
        && let Some(raw_type) =
            crate::completion::variable::raw_type_inference::resolve_array_func_raw_type(
                name,
                &func_call.argument_list,
                ctx,
            )
    {
        let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            &raw_type,
            current_class_name,
            all_classes,
            class_loader,
        );
        if !resolved.is_empty() {
            return ResolvedType::from_classes_with_hint(resolved, raw_type);
        }
        // The type string is informative (e.g. `list<User>`) but
        // doesn't resolve to a class — return as type-string-only.
        return vec![resolved_type_with_lookup(
            raw_type,
            current_class_name,
            all_classes,
            class_loader,
        )];
    }

    if let Some(ref name) = func_name
        && let Some(fl) = function_loader
        && let Some(func_info) = fl(name, func_name_offset)
    {
        // Try conditional return type first
        if let Some(ref cond) = func_info.conditional_return {
            let var_resolver = build_var_resolver_from_ctx(ctx);
            let tpl = crate::completion::types::conditional::TemplateContext::with_params(
                &func_info.template_params,
            );
            let resolved_type = resolve_conditional_with_args(
                cond,
                &func_info.parameters,
                &func_call.argument_list,
                Some(&var_resolver),
                Some(current_class_name),
                class_loader,
                &tpl,
            );
            if let Some(ref ty) = resolved_type {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    ty,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ty.clone());
                }
                // The conditional resolved to a non-class type (e.g.
                // `list<string>`, `int`).  Return it as a type-string-only
                // entry so downstream consumers see the resolved type.
                return vec![resolved_type_with_lookup(
                    ty.clone(),
                    current_class_name,
                    all_classes,
                    class_loader,
                )];
            }
        }

        // ── Function-level @template substitution ────────────
        // When the function has template params and bindings,
        // infer concrete types from the arguments and apply
        // substitution to the return type before resolving.
        if !func_info.template_params.is_empty() && func_info.return_type.is_some() {
            let arg_texts =
                crate::completion::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &func_call.argument_list,
                    content,
                );
            let rctx = ctx.as_resolution_ctx();
            let subs = build_function_template_subs(&func_info, &arg_texts, &rctx);
            if !subs.is_empty()
                && let Some(ref ret) = func_info.return_type
            {
                let substituted = ret.substitute(&subs);
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &substituted,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, substituted);
                }
                // The substituted type didn't resolve to any classes
                // (e.g. `mixed|null`, `int|null`, `array-key|null`).
                // Return it as a type-string-only entry so that
                // downstream consumers see the substituted type
                // instead of the raw template name.
                return vec![ResolvedType::from_type_string(substituted)];
            }
        }

        if let Some(ref ret) = func_info.return_type {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                ret,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, ret.clone());
            }
            // The function has a return type string but
            // `type_hint_to_classes_typed` found no matching class (e.g.
            // `list<Widget>`, `int`, `array{name: string}`).  Return a
            // type-string-only entry so that consumers reading
            // `.type_string` still get the information.
            //
            // When the return type is `void`, PHP yields `null` at
            // runtime — mirror that so the variable type is correct.
            if *ret == PhpType::void() {
                return vec![ResolvedType::from_type_string(PhpType::null())];
            }
            return vec![resolved_type_with_lookup(
                ret.clone(),
                current_class_name,
                all_classes,
                class_loader,
            )];
        }
    }

    // ── Variable invocation: $fn() ──────────────────
    // When the callee is a variable (not a named function),
    // resolve the variable's type annotation for a
    // callable/Closure return type, or look for a
    // closure/arrow-function literal in the assignment.
    if let Expression::Variable(Variable::Direct(dv)) = func_call.function {
        let var_name = bytes_to_str(dv.name).to_string();
        let offset = expr.span().start.offset as usize;

        // 1. Try docblock annotation:
        //    `@var Closure(): User $fn` or
        //    `@param callable(int): Response $fn`
        if let Some(raw_type) =
            crate::docblock::find_iterable_raw_type_in_source(content, offset, &var_name)
                .map(|t| crate::util::resolve_php_type_names(&t, class_loader))
            && let Some(ret_type) = raw_type.callable_return_type()
        {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                ret_type,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, ret_type.clone());
            }
        }

        // 2. Resolve the variable's own type.  Closures, arrow functions,
        //    and first-class callables are all inferred by
        //    `resolve_rhs_expression` as a `PhpType::Callable` (see
        //    `infer_closure_literal_type`), so `$fn`'s embedded return
        //    type covers `$fn = function(): T {}`, `$fn = fn(): T => …`,
        //    and `$fn = strlen(...)` / `$fn = $obj->method(...)` alike.
        let var_types = resolve_var_types(&var_name, ctx, ctx.cursor_offset);
        for rt in &var_types {
            if let Some(ret_type) = rt.type_string.callable_return_type() {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    ret_type,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ret_type.clone());
                }
            }
        }

        // 3. Check for __invoke().  When $f holds an object with an
        //    __invoke() method, $f() should return __invoke()'s return
        //    type.
        let var_classes = ResolvedType::into_arced_classes(var_types);
        for owner in &var_classes {
            if let Some(invoke) = owner.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    ret,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ret.clone());
                }
                // When type_hint_to_classes_typed can't resolve the return
                // type (e.g. `Item[]` where the `[]` suffix prevents
                // class lookup), emit a type-string-only entry so that
                // callers like foreach resolution can still extract the
                // element type via `PhpType::extract_value_type`.
                if !ret.is_empty() {
                    return vec![resolved_type_with_lookup(
                        ret.clone(),
                        current_class_name,
                        all_classes,
                        class_loader,
                    )];
                }
            }
        }
    }

    // ── General expression invocation: ($expr)() ────
    // When the callee is an arbitrary expression (e.g.
    // `($this->foo)()`, `(getFactory())()`, etc.), resolve
    // the expression to classes and check for __invoke().
    let callee_expr = match func_call.function {
        Expression::Parenthesized(p) => p.expression,
        other => other,
    };
    // Skip if we already handled it as a variable above.
    if !matches!(callee_expr, Expression::Variable(Variable::Direct(_))) {
        // ── Directly invoked closure / arrow function ────
        // `(fn (): Foo => …)()` or `(function (): Foo { … })()`
        // Extract the return type from the literal instead of going
        // through `__invoke()` on the generic `Closure` stub.
        if let Some(parsed_ret_type) = extract_closure_or_arrow_return_type(callee_expr) {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                &parsed_ret_type,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, parsed_ret_type);
            }
        }

        let callee_results = resolve_rhs_expression(callee_expr, ctx);
        for rt in &callee_results {
            if let Some(ref owner_cls) = rt.class_info
                && let Some(invoke) = owner_cls.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    ret,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ret.clone());
                }
                if !ret.is_empty() {
                    return vec![resolved_type_with_lookup(
                        ret.clone(),
                        current_class_name,
                        all_classes,
                        class_loader,
                    )];
                }
            }
        }
    }

    vec![]
}

/// Resolve an instance method call: `$this->method()`, `$var->method()`,
/// chained calls, and other object expressions via AST-based resolution.
/// Resolve a method call (regular or null-safe) from its constituent parts.
///
/// Both `$obj->method()` and `$obj?->method()` share the same resolution
/// logic — the null-safe operator only affects whether `null` propagates
/// at runtime, not which class the method belongs to.
pub(super) fn resolve_rhs_method_call_inner<'b>(
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let method_name = match method {
        ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
        // Variable method name (`$obj->$method()`) — can't resolve statically.
        _ => return vec![],
    };
    // Resolve the object expression to candidate owner classes.
    // Keep the full `ResolvedType` for non-$this variables and chain
    // expressions so that the receiver's generic type string (e.g.
    // `Builder<Article>`) is available when the method returns
    // `static`/`self`/`$this`.
    let (owner_classes, receiver_resolved): (Vec<Arc<ClassInfo>>, Vec<ResolvedType>) =
        if let Expression::Variable(Variable::Direct(dv)) = object
            && dv.name == b"$this"
        {
            let classes: Vec<Arc<ClassInfo>> = ctx
                .all_classes
                .iter()
                .find(|c| c.name == ctx.current_class.name)
                .map(Arc::clone)
                .into_iter()
                .collect();
            (classes, vec![])
        } else if let Expression::Variable(Variable::Direct(dv)) = object {
            let var = bytes_to_str(dv.name).to_string();
            // Check match-arm narrowing override first — when inside
            // a match(true) arm, the variable may be narrowed to a
            // specific class by the arm's instanceof condition.
            let resolved = match ctx.match_arm_narrowing.get(&var).cloned() {
                Some(overridden) => overridden,
                None => resolve_var_types(&var, ctx, object.span().end.offset),
            };
            if !resolved.is_empty() {
                let classes = ResolvedType::into_arced_classes(resolved.clone());
                (classes, resolved)
            } else {
                // Fall back to resolve_target_classes when the
                // variable resolution pipeline returns nothing (e.g.
                // for parameters that are resolved through the
                // completion pipeline's subject resolution).
                let classes: Vec<Arc<ClassInfo>> = ResolvedType::into_arced_classes(
                    crate::completion::resolver::resolve_target_classes(
                        &var,
                        crate::types::AccessKind::Arrow,
                        &ctx.as_resolution_ctx(),
                    ),
                );
                (classes, vec![])
            }
        } else {
            // Handle non-variable object expressions like
            // `(new Factory())->create()`, `getService()->method()`,
            // or chained calls by recursively resolving the expression.
            let resolved = resolve_rhs_expression(object, ctx);
            let classes = ResolvedType::into_arced_classes(resolved.clone());
            (classes, resolved)
        };

    let arg_texts = crate::completion::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
    let rctx = ctx.as_resolution_ctx();

    // ── Expand union generic receivers ──────────────────────────
    // When the receiver is a union type like `C<A>|C<B>`, the variable
    // resolution pipeline returns a single ResolvedType with a Union
    // type_string and one class_info.  To resolve the method on each
    // branch separately (so `->get()` yields `A|B` not just `A`),
    // expand the union into separate owner entries with per-branch
    // generic substitutions applied.
    let (owner_classes, receiver_resolved) =
        expand_union_generic_owners(owner_classes, receiver_resolved, ctx);

    let is_union = owner_classes.len() > 1;
    let mut union_results: Vec<ResolvedType> = Vec::new();

    for (idx, owner) in owner_classes.iter().enumerate() {
        // Build class-level template substitutions from the receiver's
        // generic type string (e.g. `Collection<int, User>` maps
        // `TKey => int, TValue => User`).  This ensures method return
        // types like `TValue` are concretised when the receiver was
        // annotated with generic arguments via `@var`.
        let class_level_subs: HashMap<String, PhpType> = receiver_resolved
            .get(idx)
            .or_else(|| receiver_resolved.first())
            .and_then(|rt| match &rt.type_string {
                PhpType::Generic(_, args)
                    if !args.is_empty()
                        && !owner.template_params.is_empty()
                        && !args.iter().any(|a| a.is_self_like()) =>
                {
                    Some(
                        owner
                            .template_params
                            .iter()
                            .zip(args.iter())
                            .map(|(name, ty)| (name.to_string(), ty.clone()))
                            .collect(),
                    )
                }
                _ => None,
            })
            .unwrap_or_default();

        let method_template_subs =
            Backend::build_method_template_subs(owner, &method_name, &arg_refs, &rctx);

        // ── @psalm-if-this-is template inference ────────────────
        // When a method has a `@psalm-if-this-is` annotation and
        // method-level template parameters remain unresolved (no
        // arguments to infer from), match the receiver's concrete
        // type against the pattern to compute substitutions.
        let if_this_is_subs: HashMap<String, PhpType> = owner
            .get_method_ci(&method_name)
            .and_then(|m| m.if_this_is.as_ref())
            .and_then(|pattern| {
                let receiver_type = receiver_resolved
                    .get(idx)
                    .or_else(|| receiver_resolved.first())
                    .map(|rt| &rt.type_string)?;
                let method = owner.get_method_ci(&method_name)?;
                Some(infer_if_this_is_subs(
                    pattern,
                    receiver_type,
                    &method.template_params,
                    &method.template_param_bounds,
                ))
            })
            .unwrap_or_default();

        // Merge class-level, method-level, and if-this-is subs.
        // if-this-is overrides method-level defaults (which may be
        // `mixed` for unresolvable templates). Method-level takes
        // precedence over class-level.
        let mut template_subs = class_level_subs;
        template_subs.extend(method_template_subs);
        template_subs.extend(if_this_is_subs);

        // When the return type contains `static`/`self`/`$this` and the
        // receiver was resolved with generic parameters, use the
        // receiver's full type (e.g. `Builder<Article>`) for
        // substitution so the generics are preserved; otherwise fall
        // back to a plain FQN swap.
        let owner_key = owner.fqn();
        let self_replace =
            |ty: &PhpType| match receiver_type_for_owner(&receiver_resolved, &owner_key) {
                Some(rt) => ty.replace_self_with_type(&rt),
                None => ty.replace_self(&owner_key),
            };

        let owner_results = resolve_owner_method_call(
            owner,
            &method_name,
            argument_list,
            ctx,
            false,
            &template_subs,
            &self_replace,
        );
        if !is_union {
            return owner_results;
        }
        ResolvedType::extend_unique(&mut union_results, owner_results);
    }

    // For intersection types, filter out `mixed` when concrete types exist.
    // When a receiver is an intersection like `IChild&IParent<C>`, each member
    // resolves the method independently: the unparameterized interface may
    // return `mixed` while the parameterized one returns `C`.  In an
    // intersection the most specific type wins, so discard `mixed` entries
    // when at least one non-mixed result is present.
    if union_results.len() > 1 {
        let has_non_mixed = union_results.iter().any(|rt| !rt.type_string.is_mixed());
        if has_non_mixed {
            union_results.retain(|rt| !rt.type_string.is_mixed());
        }
    }

    union_results
}

/// Expand union generic receiver types into separate owner entries.
///
/// When a variable has type `C<A>|C<B>`, the resolution pipeline produces
/// a single `ResolvedType` with `type_string = Union(Generic("C",[A]), Generic("C",[B]))`
/// and one `class_info` (the base class `C`).  Calling a method on such
/// a union should resolve each branch independently: `->get()` on
/// `C<A>|C<B>` where `get()` returns `T` should yield `A|B`.
///
/// This function detects such union-of-generics patterns and expands them
/// into separate owner classes, each with the appropriate template
/// substitutions applied.
pub(super) fn expand_union_generic_owners(
    owner_classes: Vec<Arc<ClassInfo>>,
    receiver_resolved: Vec<ResolvedType>,
    ctx: &VarResolutionCtx<'_>,
) -> (Vec<Arc<ClassInfo>>, Vec<ResolvedType>) {
    // Only expand when we have exactly one owner and the type_string
    // is a union with generic branches referencing the same base class.
    if owner_classes.len() != 1 || receiver_resolved.len() != 1 {
        return (owner_classes, receiver_resolved);
    }
    let rt = &receiver_resolved[0];
    let union_members = match &rt.type_string {
        PhpType::Union(members) => members,
        _ => return (owner_classes, receiver_resolved),
    };

    // Check that at least two branches are generic types of the same
    // base class, and the class has template parameters.
    let base_cls = &owner_classes[0];
    if base_cls.template_params.is_empty() {
        return (owner_classes, receiver_resolved);
    }

    let base_fqn = base_cls.fqn();
    let base_short = base_cls.name.as_str();
    let is_same_base = |name: &str| -> bool {
        name == base_short
            || name == base_fqn.as_str()
            || crate::util::short_name(name) == base_short
    };
    let generic_branches: Vec<&PhpType> = union_members
        .iter()
        .filter(|m| matches!(m, PhpType::Generic(name, _) if is_same_base(name)))
        .collect();
    if generic_branches.len() < 2 {
        return (owner_classes, receiver_resolved);
    }

    // Expand: for each generic branch, apply the type args to produce
    // a substituted ClassInfo.
    let mut expanded_owners: Vec<Arc<ClassInfo>> = Vec::new();
    let mut expanded_resolved: Vec<ResolvedType> = Vec::new();

    for member in union_members {
        match member {
            PhpType::Generic(name, args) if is_same_base(name) => {
                let arc = crate::virtual_members::resolve_class_fully_with_type_args(
                    base_cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                    args,
                );
                expanded_resolved.push(ResolvedType::from_both_arc(
                    member.clone(),
                    Arc::clone(&arc),
                ));
                expanded_owners.push(arc);
            }
            // Non-generic union members (e.g. scalars in `C<A>|int`)
            // are kept as type-string-only entries in receiver_resolved
            // but don't contribute an owner class.
            other => {
                expanded_resolved.push(ResolvedType::from_type_string(other.clone()));
            }
        }
    }

    (expanded_owners, expanded_resolved)
}

/// Find the receiver's type string that matches the given owner class name.
///
/// Scans `receiver_resolved` for a `ResolvedType` whose `class_info`
/// matches `owner_name` (short name or FQN) and whose `type_string` is a
/// `Generic` (i.e. carries generic parameters like `Builder<Article>`).
/// Returns the matching `PhpType` so that `replace_self_with_type` can
/// preserve those generic parameters when the method returns
/// `static`/`self`/`$this`.
///
/// Matching by short name alone is ambiguous for Laravel's dual
/// `Eloquent\Builder` / `Query\Builder` classes; FQN is preferred when
/// available so Query-mixin fluents like `lockForUpdate()` keep the
/// Eloquent receiver's `Builder<TModel>` type.
pub(super) fn receiver_type_for_owner(
    receiver_resolved: &[ResolvedType],
    owner_name: &str,
) -> Option<PhpType> {
    let owner_short = crate::util::short_name(owner_name);
    let mut short_match = None;
    for rt in receiver_resolved {
        let Some(ci) = rt.class_info.as_ref() else {
            continue;
        };
        if !matches!(rt.type_string, PhpType::Generic(_, _)) {
            continue;
        }
        if ci.fqn().as_str() == owner_name || ci.name.as_str() == owner_name {
            return Some(rt.type_string.clone());
        }
        if short_match.is_none() && ci.name.as_str() == owner_short {
            short_match = Some(rt.type_string.clone());
        }
    }
    short_match
}

/// Resolve a method's PHPStan conditional return type against the call-site
/// arguments, returning the winning branch's type when it is definite and
/// informative.
///
/// The returned type has template substitutions applied, `self`/`static`/
/// `$this` replaced (via the `replace_self` closure, which differs between the
/// instance and static call paths), and any conditionals nested inside the
/// winning branch collapsed.  Returns `None` when the method has no
/// conditional return type, the condition cannot be decided from the
/// arguments, or the winning branch is uninformative (a bare `mixed`/`array`
/// else-branch) — in which case the caller falls back to the native return
/// type so the full union (including scalar/`array` members) is preserved.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_conditional_return_for_call(
    method_ref: Option<&crate::types::MethodInfo>,
    text_args: &str,
    var_resolver: crate::completion::conditional_resolution::VarClassStringResolver<'_>,
    calling_class_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    template_subs: &HashMap<String, PhpType>,
    arg_type_resolver: crate::completion::conditional_resolution::ArgTypeResolver<'_>,
    replace_self: impl Fn(&PhpType) -> PhpType,
) -> Option<PhpType> {
    let method = method_ref?;
    let cond = method.conditional_return.as_ref()?;
    let params = method.parameters.as_slice();
    let tpl = crate::completion::conditional_resolution::TemplateContext {
        defaults: None,
        params: method.template_params.as_slice(),
        arg_type_resolver,
    };
    let resolved =
        crate::completion::conditional_resolution::resolve_conditional_with_text_args_and_defaults(
            cond,
            params,
            text_args,
            var_resolver,
            Some(calling_class_name),
            class_loader,
            &tpl,
        )?;
    let substituted = if template_subs.is_empty() {
        resolved
    } else {
        resolved.substitute(template_subs)
    };
    let substituted = if substituted.contains_self_ref() {
        replace_self(&substituted)
    } else {
        substituted
    };
    // Collapse any conditionals nested inside the winning branch.
    let collapsed = if substituted.contains_conditional() {
        let tpl2 = crate::completion::conditional_resolution::TemplateContext {
            defaults: Some(template_subs),
            params: method.template_params.as_slice(),
            arg_type_resolver,
        };
        crate::completion::conditional_resolution::evaluate_nested_conditionals_text(
            &substituted,
            params,
            text_args,
            var_resolver,
            Some(calling_class_name),
            class_loader,
            &tpl2,
        )
    } else {
        substituted
    };
    if collapsed.is_uninformative_return() {
        None
    } else {
        Some(collapsed)
    }
}

/// Resolve an authoritative return type (e.g. a call-site-narrowed
/// conditional branch) to `ResolvedType` values.
///
/// Prefers class-backed results when the type names concrete classes, keeping
/// the full type string as the hint (so generics like `Collection<int, User>`
/// survive).  When the type names no class (a bare `array<…>`, `list<…>`,
/// scalar, or shape) a type-string-only entry is returned so consumers that
/// read `.type_string` still see it.  `void` collapses to `null`.
pub(super) fn resolve_from_authoritative_type(
    ty: PhpType,
    current_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<ResolvedType> {
    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        &ty,
        current_class_name,
        all_classes,
        class_loader,
    );
    if !classes.is_empty() {
        return ResolvedType::from_classes_with_hint(classes, ty);
    }
    if ty == PhpType::void() {
        return vec![ResolvedType::from_type_string(PhpType::null())];
    }
    vec![resolved_type_with_lookup(
        ty,
        current_class_name,
        all_classes,
        class_loader,
    )]
}

/// Resolve a method call's return type against a single, fully determined
/// owner class: template substitution, `@psalm-if-this-is` narrowing (via
/// the caller-supplied `template_subs`), PHPStan conditional return types,
/// and body-return-type inference, in that order.
///
/// Shared by instance method calls (called once per union-receiver branch)
/// and static method calls (a single owner, no receiver-derived generics).
/// `self_replace` maps `static`/`self`/`$this` in a resolved return type to
/// the owner's concrete type: generic-aware (via [`receiver_type_for_owner`])
/// for instance calls, a plain FQN swap for static calls, which have no
/// receiver expression to carry generics.
pub(super) fn resolve_owner_method_call(
    owner: &ClassInfo,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
    is_static: bool,
    template_subs: &HashMap<String, PhpType>,
    self_replace: &dyn Fn(&PhpType) -> PhpType,
) -> Vec<ResolvedType> {
    let current_class_name: &str = &ctx.current_class.name;
    let arg_texts = crate::completion::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let text_args = arg_texts.join(", ");
    let rctx = ctx.as_resolution_ctx();
    let var_resolver = build_var_resolver_from_ctx(ctx);
    let mr_ctx = MethodReturnCtx {
        all_classes: ctx.all_classes,
        class_loader: ctx.class_loader,
        template_subs,
        var_resolver: Some(&var_resolver),
        cache: ctx.resolved_class_cache,
        calling_class_name: Some(&ctx.current_class.name),
        is_static,
    };

    // Try the owner directly first — it may already be fully resolved with
    // generic substitutions applied.  The cache is keyed by bare FQN and
    // returns the un-substituted base class, so prefer the owner's own
    // method to preserve template substitutions.
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        owner,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    if let Some((date_class, date_return_type)) =
        Backend::configured_laravel_date_return(&merged, method_name, ctx.class_loader)
    {
        return ResolvedType::from_classes_with_hint(vec![date_class], date_return_type);
    }

    let owner_method = owner.get_method_ci(method_name);
    let merged_method = merged.get_method_ci(method_name);
    // Prefer the merged method's return type when the owner's method has no
    // docblock override (return_type == native_return_type).  The merged
    // method carries inherited types from interfaces/parents with template
    // substitutions already applied (e.g. `V|null` → `User|null` from
    // `@implements Collection<string, User>`).
    let method_ref = match (owner_method, merged_method) {
        (Some(om), Some(mm))
            if om.return_type == om.native_return_type
                && mm.return_type != mm.native_return_type =>
        {
            Some(mm)
        }
        (Some(om), _) => Some(om),
        (None, Some(mm)) => Some(mm),
        // Method not found — fall back to the magic method's return type.
        (None, None) => merged.get_method_ci(if is_static { "__callStatic" } else { "__call" }),
    };
    // Recover the effective return type string from the method and replace
    // `static`/`self`/`$this` with the owner's concrete type so that e.g.
    // `static[]` becomes `Country[]`.
    let native_ret_type_string = method_ref.and_then(|m| m.return_type.as_ref()).map(|ret| {
        let substituted = if !template_subs.is_empty() {
            ret.substitute(template_subs).simplified()
        } else {
            ret.clone()
        };
        // Resolve `parent` to the concrete parent class name before any
        // self/static replacement so that downstream consumers see a real
        // FQN instead of the keyword.
        let substituted = if substituted.is_parent_ref() {
            owner
                .parent_class
                .as_ref()
                .map(|p| PhpType::Named(p.to_string()))
                .unwrap_or(substituted)
        } else {
            substituted
        };
        if substituted.contains_self_ref() {
            self_replace(&substituted)
        } else {
            substituted
        }
    });

    // Resolver from an argument's source text to its type, used to evaluate
    // `is <Type>` conditions whose argument is an expression (a method-call
    // chain, property access, …) rather than a literal.
    let arg_ty_resolver = |t: &str| Backend::resolve_arg_text_to_type(t, &rctx);

    // Resolve the PHPStan conditional return type against the call-site
    // arguments, if the method declares one.  When it yields an informative
    // type it is *authoritative*: the branch it selects (e.g.
    // `list<\stdClass>` from `PDOStatement::fetchAll`, or `array<TKey,
    // static>` for a literal-array argument) supersedes the method's broad
    // native union return type.  Resolving classes from the native union
    // instead would both ignore the call-site narrowing and silently drop
    // scalar or `array` members the union carries.
    let conditional_ret = resolve_conditional_return_for_call(
        method_ref,
        &text_args,
        Some(&var_resolver),
        current_class_name,
        ctx.class_loader,
        template_subs,
        Some(&arg_ty_resolver),
        self_replace,
    );

    // Collapse any conditionals nested inside the (template-substituted)
    // native return type against the call arguments, so a generic wrapper
    // like `Collection<($groupBy is array|string ? array-key : …), …>`
    // yields a concrete key type instead of carrying a raw conditional that
    // later gets compared against — and printed in — an argument-type
    // diagnostic.
    let native_ret_type_string = native_ret_type_string.map(|ty| {
        if ty.contains_conditional() {
            let params = method_ref.map(|m| m.parameters.as_slice()).unwrap_or(&[]);
            let tpl = crate::completion::conditional_resolution::TemplateContext {
                defaults: Some(template_subs),
                params: method_ref
                    .map(|m| m.template_params.as_slice())
                    .unwrap_or(&[]),
                arg_type_resolver: Some(&arg_ty_resolver),
            };
            crate::completion::conditional_resolution::evaluate_nested_conditionals_text(
                &ty,
                params,
                &text_args,
                Some(&var_resolver),
                Some(current_class_name),
                ctx.class_loader,
                &tpl,
            )
        } else {
            ty
        }
    });

    // When the conditional resolved to a definite, informative type, it
    // wins — resolve the result classes from it directly.
    if let Some(cond_ty) = conditional_ret {
        return resolve_from_authoritative_type(
            cond_ty,
            current_class_name,
            ctx.all_classes,
            ctx.class_loader,
        );
    }

    let ret_type_string = native_ret_type_string;

    let results =
        Backend::resolve_method_return_types_with_args(owner, method_name, &text_args, &mr_ctx);
    if !results.is_empty() {
        return match ret_type_string {
            Some(hint) => ResolvedType::from_classes_with_hint(results, hint),
            None => ResolvedType::from_classes(results),
        };
    }

    // The method has a return type string but `type_hint_to_classes_typed`
    // found no matching class (e.g. `list<Widget>`, `int`, `array{name:
    // string}`).  Return a type-string-only entry so that consumers reading
    // `.type_string` (hover, foreach resolution, null-coalesce stripping)
    // still get the information.
    //
    // Return the type string even for non-informative types like `array` or
    // `mixed` — a correct-but-vague type is better than keeping the
    // previous (wrong) type after reassignment.  Skip only `void` (void
    // methods don't produce a value).  Also expand type aliases before
    // returning so that `@phpstan-type UserList array<int, User>` with
    // `@return UserList` is expanded to its concrete type.
    if let Some(hint) = ret_type_string {
        let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
            &hint,
            &owner.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        let parsed_effective = expanded.unwrap_or(hint);
        if parsed_effective == PhpType::void() {
            return vec![ResolvedType::from_type_string(PhpType::null())];
        }
        return vec![resolved_type_with_lookup(
            parsed_effective,
            current_class_name,
            ctx.all_classes,
            ctx.class_loader,
        )];
    }

    // Body return type inference fallback: when the method has no declared
    // return type and no @return docblock, try to infer the return type
    // from the method body.  This handles non-class types (list<Foo>, int,
    // array shapes) that resolve_method_return_types_with_args cannot
    // represent.
    if method_ref.is_some_and(|m| m.return_type.is_none() && m.name_offset != 0 && !m.is_virtual)
        && let Some(inferred) = crate::completion::call_resolution::try_infer_body_return_type(
            &owner.fqn(),
            method_ref.unwrap(),
        )
        && !inferred.is_void()
        && !inferred.is_mixed()
    {
        return vec![resolved_type_with_lookup(
            inferred,
            current_class_name,
            ctx.all_classes,
            ctx.class_loader,
        )];
    }

    vec![]
}

/// Resolve a static method call: `ClassName::method()`, `self::method()`,
/// `static::method()`.
pub(super) fn resolve_rhs_static_call(
    static_call: &StaticMethodCall<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class_name: &str = &ctx.current_class.name;

    let class_name = match static_call.class {
        Expression::Self_(_) => Some(current_class_name.to_string()),
        Expression::Static(_) => Some(current_class_name.to_string()),
        Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        // ── `$var::method()` where `$var` holds a class-string ──
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name).to_string();
            let targets =
                crate::completion::variable::class_string_resolution::resolve_class_string_targets(
                    &var_name,
                    ctx.current_class,
                    ctx.all_classes,
                    ctx.content,
                    ctx.cursor_offset,
                    ctx.class_loader,
                );
            // When there are multiple possible class targets (union class-string),
            // resolve the method return type through each and union the results.
            if targets.len() > 1 {
                if let ClassLikeMemberSelector::Identifier(ident) = &static_call.method {
                    let method_name_str = bytes_to_str(ident.value).to_string();
                    let mut union_types: Vec<PhpType> = Vec::new();
                    let mut union_classes: Vec<ResolvedType> = Vec::new();
                    for target in &targets {
                        let arg_texts = crate::completion::variable::raw_type_inference::extract_arg_texts_from_ast(
                            &static_call.argument_list,
                            ctx.content,
                        );
                        let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
                        let text_args = arg_texts.join(", ");
                        let rctx = ctx.as_resolution_ctx();
                        let template_subs = Backend::build_method_template_subs(
                            target,
                            &method_name_str,
                            &arg_refs,
                            &rctx,
                        );
                        let var_resolver = build_var_resolver_from_ctx(ctx);
                        let mr_ctx = MethodReturnCtx {
                            all_classes: ctx.all_classes,
                            class_loader: ctx.class_loader,
                            template_subs: &template_subs,
                            var_resolver: Some(&var_resolver),
                            cache: ctx.resolved_class_cache,
                            calling_class_name: Some(&ctx.current_class.name),
                            is_static: true,
                        };
                        // Get the method's return type string.
                        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                            target,
                            ctx.class_loader,
                            ctx.resolved_class_cache,
                        );
                        let method_ref = target
                            .get_method_ci(&method_name_str)
                            .or_else(|| merged.get_method_ci(&method_name_str));
                        if let Some(m) = method_ref {
                            if let Some(ref ret) = m.return_type {
                                let substituted = if !template_subs.is_empty() {
                                    ret.substitute(&template_subs)
                                } else {
                                    ret.clone()
                                };
                                let resolved = substituted.replace_self(&target.fqn());
                                union_types.push(resolved);
                            }
                        } else {
                            // Try to resolve through resolve_method_return_types_with_args
                            let results = Backend::resolve_method_return_types_with_args(
                                target,
                                &method_name_str,
                                &text_args,
                                &mr_ctx,
                            );
                            for r in results {
                                union_classes.push(ResolvedType::from_both_arc(
                                    PhpType::Named(r.name.to_string()),
                                    r,
                                ));
                            }
                        }
                    }
                    if !union_types.is_empty() || !union_classes.is_empty() {
                        // Build a unified type from all resolved return types.
                        let combined = if union_types.len() == 1 && union_classes.is_empty() {
                            union_types.remove(0)
                        } else if union_types.is_empty() && !union_classes.is_empty() {
                            return union_classes;
                        } else {
                            PhpType::Union(union_types)
                        };
                        let resolved_classes =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &combined,
                                current_class_name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved_classes.is_empty() {
                            return ResolvedType::from_classes_with_hint(
                                resolved_classes,
                                combined,
                            );
                        }
                        return vec![ResolvedType::from_type_string(combined)];
                    }
                }
                // Fallback: use first target.
                return vec![];
            }
            if let Some(first) = targets.first() {
                Some(first.name.to_string())
            } else {
                // Fallback: resolve the variable's type and extract the
                // inner type from `class-string<T>`.  This handles
                // parameters typed as `@param class-string<Foo> $var`
                // where there is no `$var = Foo::class` assignment.
                let resolved = resolve_var_types(&var_name, ctx, ctx.cursor_offset);
                resolved
                    .iter()
                    .find_map(|rt| match &rt.type_string {
                        PhpType::ClassString(Some(inner)) => {
                            inner.base_name().map(|s| s.to_string())
                        }
                        PhpType::Nullable(inner) => match inner.as_ref() {
                            PhpType::ClassString(Some(cs_inner)) => {
                                cs_inner.base_name().map(|s| s.to_string())
                            }
                            _ => None,
                        },
                        PhpType::Union(members) => members.iter().find_map(|m| match m {
                            PhpType::ClassString(Some(inner)) => {
                                inner.base_name().map(|s| s.to_string())
                            }
                            PhpType::Nullable(inner) => match inner.as_ref() {
                                PhpType::ClassString(Some(cs_inner)) => {
                                    cs_inner.base_name().map(|s| s.to_string())
                                }
                                _ => None,
                            },
                            _ => None,
                        }),
                        _ => None,
                    })
                    .or_else(|| {
                        // Final fallback: `$var::method()` where `$var` is an
                        // object instance (not a class-string). In PHP you can
                        // call static methods on an instance reference.
                        resolved
                            .iter()
                            .find_map(|rt| rt.type_string.base_name().map(|s| s.to_string()))
                    })
            }
        }
        _ => None,
    };
    if let Some(cls_name) = class_name
        && let ClassLikeMemberSelector::Identifier(ident) = &static_call.method
    {
        let method_name = bytes_to_str(ident.value).to_string();
        let owner = (ctx.class_loader)(&cls_name)
            .map(Arc::unwrap_or_clone)
            .or_else(|| {
                ctx.all_classes
                    .iter()
                    .find(|c| c.name == cls_name)
                    .map(|c| ClassInfo::clone(c))
            });
        if let Some(ref owner) = owner {
            let concrete_owner = if !class_has_method(
                owner,
                &method_name,
                ctx.class_loader,
                ctx.resolved_class_cache,
            ) {
                facade_accessor_concrete_owner(
                    owner,
                    &method_name,
                    ctx.content,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                )
            } else {
                None
            };
            let owner = concrete_owner.as_ref().unwrap_or(owner);

            let arg_texts =
                crate::completion::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &static_call.argument_list,
                    ctx.content,
                );
            let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
            let rctx = ctx.as_resolution_ctx();
            let template_subs =
                Backend::build_method_template_subs(owner, &method_name, &arg_refs, &rctx);
            let owner_key = owner.fqn();
            let self_replace = |ty: &PhpType| ty.replace_self(&owner_key);

            return resolve_owner_method_call(
                owner,
                &method_name,
                &static_call.argument_list,
                ctx,
                true,
                &template_subs,
                &self_replace,
            );
        }
    }
    vec![]
}

fn class_has_method(
    class_info: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&crate::virtual_members::ResolvedClassCache>,
) -> bool {
    if class_info.get_method_ci(method_name).is_some() {
        return true;
    }
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class_info, class_loader, cache);
    merged.get_method_ci(method_name).is_some()
}

fn facade_accessor_concrete_owner(
    facade: &ClassInfo,
    method_name: &str,
    content: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&crate::virtual_members::ResolvedClassCache>,
) -> Option<ClassInfo> {
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(facade, class_loader, cache);

    if let Some(concrete) = crate::virtual_members::laravel::parse_facade_accessor(content)
        .and_then(facade_accessor_to_class_name)
        .and_then(|name| class_loader(&name))
        .filter(|class| class_has_method(class, method_name, class_loader, cache))
    {
        return Some(Arc::unwrap_or_clone(concrete));
    }

    if let Some(accessor) = facade
        .get_method_ci("getFacadeAccessor")
        .or_else(|| merged.get_method_ci("getFacadeAccessor"))
        && let Some(inferred) =
            crate::completion::call_resolution::try_infer_body_return_type(&facade.fqn(), accessor)
        && let Some(concrete_name) = facade_accessor_class_name(&inferred)
        && let Some(concrete) = class_loader(&concrete_name)
        && class_has_method(&concrete, method_name, class_loader, cache)
    {
        return Some(Arc::unwrap_or_clone(concrete));
    }

    facade
        .mixins
        .iter()
        .chain(merged.mixins.iter())
        .find_map(|mixin| {
            let class = class_loader(mixin.as_str())?;
            class_has_method(&class, method_name, class_loader, cache)
                .then(|| Arc::unwrap_or_clone(class))
        })
}

fn facade_accessor_class_name(ty: &PhpType) -> Option<String> {
    match ty {
        PhpType::ClassString(Some(inner)) => inner.base_name().map(ToString::to_string),
        PhpType::Named(name) => Some(name.to_string()),
        _ => None,
    }
}

fn facade_accessor_to_class_name(
    accessor: crate::virtual_members::laravel::FacadeAccessor,
) -> Option<String> {
    match accessor {
        crate::virtual_members::laravel::FacadeAccessor::Class(name) => Some(name),
        crate::virtual_members::laravel::FacadeAccessor::Alias(_) => None,
    }
}
