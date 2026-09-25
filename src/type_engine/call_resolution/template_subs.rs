/// Template substitution for method-level `@template` parameters: builds
/// a substitution map from call-site argument texts and resolves closure
/// return types against bound template parameters.
use std::collections::HashMap;

use crate::Backend;
use crate::atom::{Atom, AtomSet, atom};
use crate::class_lookup::is_self_or_static;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::variable::rhs_resolution::{
    TemplateBindingMode, classify_template_binding, extract_array_position,
};
use crate::types::*;

use crate::type_engine::resolver::{Loaders, ResolutionCtx};

use super::return_types::{
    resolve_call_return_hint, resolve_cast_type, resolve_chain_declared_return,
    resolve_expression_to_type, resolve_literal_type, resolve_operator_type,
    resolve_static_access_type,
};

impl Backend {
    /// Build a template substitution map for a method-level `@template` call.
    ///
    /// Finds the method on the class (or inherited), checks for template
    /// params and bindings, resolves argument types from the pre-split
    /// `arg_texts` slice using the call resolution context, and returns a
    /// `HashMap` mapping template parameter names to their resolved
    /// concrete types.
    ///
    /// Callers with an AST `ArgumentList` should extract per-argument text
    /// via [`extract_arg_texts_from_ast`] and convert to `&[&str]`.
    /// Callers with only raw text should use [`split_text_args`] first.
    ///
    /// Returns an empty map if the method has no template params, no
    /// bindings, or if argument types cannot be resolved.
    pub(crate) fn build_method_template_subs(
        class_info: &ClassInfo,
        method_name: &str,
        arg_texts: &[&str],
        ctx: &ResolutionCtx<'_>,
    ) -> HashMap<String, PhpType> {
        // Find the method — first on the class directly, then via inheritance.
        let method = class_info.get_method(method_name).cloned().or_else(|| {
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                class_info,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            merged.get_method(method_name).cloned()
        });

        let method = match method {
            Some(m) if !m.template_params.is_empty() => m,
            _ => return HashMap::new(),
        };

        let mut subs = Self::bind_method_template_args(&method, arg_texts, ctx);

        finish_template_subs(
            &mut subs,
            &method.template_params,
            &method.template_param_bounds,
            method.return_type.as_ref(),
            ctx,
        );

        subs
    }

    /// The argument-binding half of [`build_method_template_subs`]: resolve
    /// each of `method`'s `@template` bindings from `arg_texts` and return
    /// the map, without filling in the params nothing bound.
    ///
    /// A caller that has its own idea of what an unbound param should fall
    /// back to (e.g. `new` binding a *class*-level template through an
    /// inherited constructor, where the right bound lives on the class,
    /// not the constructor) calls this directly instead of
    /// [`build_method_template_subs`], which always finishes against the
    /// method's own bounds.
    pub(crate) fn bind_method_template_args(
        method: &MethodInfo,
        arg_texts: &[&str],
        ctx: &ResolutionCtx<'_>,
    ) -> HashMap<String, PhpType> {
        let mut subs = HashMap::new();

        // Bind the raw source-order argument texts to parameters by PHP's
        // rules so a named argument (`id: Foo::class`) is routed to the
        // parameter it targets rather than its ordinal slot, and its `name:`
        // prefix is stripped off the value.
        let bound = crate::call_args::bind_text_args_to_params(&method.parameters, arg_texts);

        for (tpl_name, param_name) in &method.template_bindings {
            let param_idx = match method
                .parameters
                .iter()
                .position(|p| p.name == param_name.as_str())
            {
                Some(idx) => idx,
                None => continue,
            };

            // Classify how the template param appears in the parameter's
            // type hint (direct, array element, generic wrapper, or
            // callable return type).
            let param_hint = method
                .parameters
                .get(param_idx)
                .and_then(|p| p.type_hint.as_ref());
            let binding_mode = classify_template_binding(tpl_name, param_hint);

            let tpl_bound = method.template_param_bounds.get(&atom(tpl_name));

            let arg_text = match bound.get(param_idx).and_then(|o| o.as_deref()) {
                Some(text) => text,
                None => {
                    let default_value = method
                        .parameters
                        .get(param_idx)
                        .and_then(|p| p.default_value.as_deref());
                    // A template bounded by a type operator resolves
                    // against the one literal it binds to, and an omitted
                    // argument has such a literal whenever the parameter
                    // declares a scalar default — known at the declaration
                    // site exactly as an explicit argument is known at the
                    // call site.
                    match default_value {
                        Some(d)
                            if !subs.contains_key(tpl_name.as_str())
                                && type_operator_bound_literal(tpl_bound, d).is_some() =>
                        {
                            d
                        }
                        _ => match &binding_mode {
                            TemplateBindingMode::ClassStringInner => match default_value {
                                Some(d) if !subs.contains_key(tpl_name.as_str()) => d,
                                None => continue,
                                _ => continue,
                            },
                            TemplateBindingMode::Direct => match default_value {
                                Some(d)
                                    if !subs.contains_key(tpl_name.as_str())
                                        && (d == "null" || d.ends_with("::class")) =>
                                {
                                    d
                                }
                                _ => continue,
                            },
                            _ => continue,
                        },
                    }
                }
            };

            if let Some(literal) = type_operator_bound_literal(tpl_bound, arg_text) {
                crate::type_engine::variable::rhs_resolution::insert_or_union(
                    &mut subs,
                    tpl_name.to_string(),
                    literal,
                );
                continue;
            }

            match binding_mode {
                TemplateBindingMode::Direct => {
                    if let Some(resolved_type) = Self::resolve_arg_text_to_type(arg_text, ctx) {
                        // `resolve_arg_text_to_type` collapses any `[...]`
                        // literal to the bare `array` keyword, which loses
                        // the argument's own keys. When the template binds
                        // directly (e.g. `@template T of array<array-key,
                        // mixed>` with `@param T $items`), that erased
                        // shape is the only source of type information —
                        // there is no wrapping hint to unify against — so
                        // build the literal's real key/value shape here
                        // instead, letting `key-of<T>`/`value-of<T>` on the
                        // bound template project the caller's actual keys.
                        let literal_shape = resolved_type
                            .is_bare_array()
                            .then(|| array_literal_shape_type(arg_text, ctx))
                            .flatten();

                        // `Direct` is also where the classifier lands for a
                        // hint that buries the template deeper than it
                        // models (`array<string, array<string, T>>`).
                        // Binding the whole argument there would re-wrap it,
                        // so unify the two shapes when the hint is not just
                        // the template name.
                        let unify_hint = param_hint.filter(
                            |h| !matches!(h.kind(), TypeKind::Named(n) if &**n == tpl_name.as_str()),
                        );
                        let bound_type = unify_hint
                            .and_then(|h| {
                                // A union hint that offers an array-like
                                // alternative alongside the bare template
                                // name (`iterable<array-key, T>|T`) still
                                // classifies as `Direct`, because the bare
                                // alternative matches any argument. An
                                // array *literal* argument resolves to a
                                // bare `array` with no element type
                                // though, so unifying against it falls
                                // through to `mixed` — unwrap the
                                // literal's first element the same way
                                // `GenericWrapper` binding does and retry
                                // before that fallback.
                                if resolved_type.is_bare_array()
                                    && let Some(elem) =
                                        first_array_literal_element_type(arg_text, ctx)
                                    && let Some(unified) =
                                        unify_template(h, &PhpType::array_of(elem), tpl_name)
                                {
                                    return Some(unified);
                                }
                                unify_template(h, &resolved_type, tpl_name)
                            })
                            .or(literal_shape)
                            .unwrap_or(resolved_type);
                        crate::type_engine::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            bound_type,
                        );
                    }
                }
                TemplateBindingMode::GenericWrapper(ref wrapper_name, tpl_position) => {
                    // When the argument is a closure and the param hint
                    // union contains a Callable variant (e.g.
                    // `iterable<T>|(Closure(): Generator<T>)`), try yield
                    // inference first — before array-like or hierarchy
                    // extraction, which would incorrectly bind `Closure`.
                    if let Some(concrete) = Self::try_closure_return_type_for_template(
                        arg_text,
                        tpl_name,
                        tpl_position,
                        param_hint,
                        ctx,
                    ) {
                        crate::type_engine::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            concrete,
                        );
                        continue;
                    }

                    // For array-like wrappers (`array<T>`, `list<T>`, etc.)
                    // resolve the argument to its array type and extract the
                    // positional generic argument.
                    //
                    // `classify_template_binding` assigns positions by index
                    // in the generic args list: `array<T>` → position 0,
                    // `array<TKey, TValue>` → positions 0 and 1.  For
                    // single-param `array<T>`, T is semantically the
                    // *value* type even though it sits at index 0.  We
                    // detect this by checking the param hint's generic
                    // args count: if there's only one arg, position 0
                    // maps to the value type; otherwise position 0 is the
                    // key type and position 1 is the value type.
                    if crate::type_engine::variable::rhs_resolution::is_array_like_wrapper(
                        wrapper_name,
                    ) {
                        // Array literal: `[1, 2, 3]` — resolve individual
                        // elements to infer the element type.
                        // `resolve_arg_text_to_type("[1, 2, 3]")` returns
                        // bare `array` (no generics), so we must unwrap the
                        // literal and resolve the first element directly.
                        if arg_text.starts_with('[') && arg_text.ends_with(']') {
                            if let Some(resolved_elem) =
                                first_array_literal_element_type(arg_text, ctx)
                            {
                                crate::type_engine::variable::rhs_resolution::insert_or_union(
                                    &mut subs,
                                    tpl_name.to_string(),
                                    resolved_elem,
                                );
                            }
                            continue;
                        }

                        // Variable or expression argument: resolve to a
                        // typed value and extract the positional generic
                        // argument (key or value type).
                        if let Some(resolved_type) = Self::resolve_arg_text_to_type(arg_text, ctx) {
                            // Walk the parameter hint and the argument type
                            // together first.  Positional extraction only
                            // unwraps one level, so it binds the whole inner
                            // array for a hint like
                            // `array<string, array<string, T>>`.
                            if let Some(unified) = param_hint
                                .filter(|h| !names_template_directly(h, tpl_name))
                                .and_then(|h| unify_template(h, &resolved_type, tpl_name))
                            {
                                crate::type_engine::variable::rhs_resolution::insert_or_union(
                                    &mut subs,
                                    tpl_name.to_string(),
                                    unified,
                                );
                                continue;
                            }
                            let generic_arg_count = param_hint
                                .and_then(|h| match h.kind() {
                                    crate::php_type::TypeKind::Generic(g) => Some(g.args.len()),
                                    _ => None,
                                })
                                .unwrap_or(1);

                            let concrete = if generic_arg_count <= 1 {
                                // Single-param: `array<T>`, `list<T>` — T is the value/element type.
                                resolved_type.extract_value_type(false).cloned()
                            } else {
                                match tpl_position {
                                    0 => resolved_type.extract_key_type(false).cloned(),
                                    1 => resolved_type.extract_value_type(false).cloned(),
                                    _ => None,
                                }
                            };
                            if let Some(concrete) = concrete {
                                crate::type_engine::variable::rhs_resolution::insert_or_union(
                                    &mut subs,
                                    tpl_name.to_string(),
                                    concrete,
                                );
                            } else {
                                crate::type_engine::variable::rhs_resolution::insert_or_union(
                                    &mut subs,
                                    tpl_name.to_string(),
                                    resolved_type,
                                );
                            }
                        }
                        continue;
                    }

                    if let Some(resolved_type) = Self::resolve_arg_text_to_type(arg_text, ctx) {
                        // Special handling for class-string<T> to avoid double-wrapping
                        if wrapper_name == "class-string"
                            && tpl_position == 0
                            && let Some(inner) = resolved_type.unwrap_class_string_inner()
                        {
                            crate::type_engine::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                inner.clone(),
                            );
                            continue;
                        }

                        // For non-array-like generic wrappers (e.g.
                        // `Iterator<T>`, `Traversable<T>`), try to
                        // extract the positional generic arg through
                        // the class hierarchy.  When the argument type
                        // is a class that implements/extends the wrapper
                        // interface with concrete generic args, use
                        // those args instead of the raw class name.
                        //
                        // 1. If the resolved type is itself Generic with
                        //    a matching wrapper name, extract directly.
                        // 2. Otherwise resolve the type to a class and
                        //    check implements_generics / extends_generics
                        //    for the wrapper interface.
                        let extracted = (|| -> Option<PhpType> {
                            // Direct match: resolved type is already
                            // `Wrapper<..., ConcreteArg, ...>`.
                            if let TypeKind::Generic(g) = &resolved_type.kind() {
                                let args = &g.args;
                                let short = crate::util::short_name(&g.name);
                                let wrapper_short = crate::util::short_name(wrapper_name);
                                if short == wrapper_short {
                                    // When the param hint has fewer
                                    // generic args than the resolved
                                    // type (e.g. `Iterator<T>` vs
                                    // `Iterator<int, ASTClass>`), the
                                    // single param-hint arg represents
                                    // the value/last type.
                                    let param_generic_count = param_hint
                                        .and_then(|h| match h.kind() {
                                            TypeKind::Generic(g) => Some(g.args.len()),
                                            _ => None,
                                        })
                                        .unwrap_or(1);
                                    if param_generic_count == 1 && args.len() > 1 {
                                        return args.last().cloned();
                                    }
                                    return args.get(tpl_position).cloned();
                                }
                            }

                            // Hierarchy lookup: resolve the type to a
                            // class and search its implements_generics
                            // and extends_generics for the wrapper.
                            let base_name = resolved_type.base_name()?;
                            let cls = (ctx.class_loader)(base_name)?;
                            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                                &cls,
                                ctx.class_loader,
                                ctx.resolved_class_cache,
                            );
                            let wrapper_short = crate::util::short_name(wrapper_name);

                            // Build a substitution map from the class's
                            // template params to the concrete generic
                            // args from the resolved type.  E.g. when
                            // the resolved type is
                            // `ASTArtifactList<ASTClass>` and the class
                            // declares `@template T of ASTArtifact`,
                            // this maps `T → ASTClass`.  Without this,
                            // the `@implements Iterator<int|string, T>`
                            // would return the raw `T` instead of the
                            // concrete `ASTClass`.
                            let class_tpl_subs: HashMap<String, PhpType> =
                                if let TypeKind::Generic(g) = &resolved_type.kind() {
                                    merged
                                        .template_params
                                        .iter()
                                        .zip(g.args.iter())
                                        .map(|(name, ty)| (name.to_string(), ty.clone()))
                                        .collect()
                                } else {
                                    HashMap::new()
                                };

                            for (iface_name, args) in merged
                                .implements_generics
                                .iter()
                                .chain(merged.extends_generics.iter())
                            {
                                let iface_short = crate::util::short_name(iface_name);
                                if iface_short != wrapper_short {
                                    continue;
                                }
                                if args.is_empty() {
                                    continue;
                                }

                                // Apply class-level template subs so
                                // that e.g. `Iterator<int|string, T>`
                                // becomes `Iterator<int|string, ASTClass>`.
                                let args: Vec<PhpType> = if !class_tpl_subs.is_empty() {
                                    args.iter().map(|a| a.substitute(&class_tpl_subs)).collect()
                                } else {
                                    args.clone()
                                };

                                let param_generic_count = param_hint
                                    .and_then(|h| match h.kind() {
                                        TypeKind::Generic(g) => Some(g.args.len()),
                                        _ => None,
                                    })
                                    .unwrap_or(1);
                                // When the @param hint has a single
                                // generic arg but the @implements
                                // clause has multiple, the single arg
                                // represents the value (last) type.
                                if param_generic_count == 1 && args.len() > 1 {
                                    return args.last().cloned();
                                }
                                return args.get(tpl_position).cloned();
                            }

                            None
                        })();

                        if let Some(concrete) = extracted {
                            crate::type_engine::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                concrete,
                            );
                        } else {
                            // The closure-return-type fallback for union
                            // param hints like `iterable<T>|(Closure(): T)`
                            // already ran at the top of this branch, so a
                            // failed extraction here binds the resolved arg
                            // type directly.
                            crate::type_engine::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                resolved_type,
                            );
                        }
                    }
                }
                TemplateBindingMode::CallableReturnType => {
                    if let Some(bound) =
                        bind_callable_return_template(arg_text, param_hint, tpl_name, ctx)
                    {
                        crate::type_engine::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            bound,
                        );
                    }
                }
                TemplateBindingMode::CallableReturnArrayPosition(position) => {
                    // `@param callable(...): array<TKey, TValue> $cb`
                    // (`mapWithKeys()`, `mapToGroups()`) — bind from the
                    // key (0) or value (1) of the callback's array-shaped
                    // return, not the whole return type. A bare `: array`
                    // annotation carries no key/value information, so
                    // fall back to the literal array the body returns
                    // (e.g. `fn ($o): array => ['x' => $o]`).
                    let extracted = Self::infer_closure_return_type(arg_text, ctx)
                        .and_then(|ret_type| extract_array_position(&ret_type, position))
                        .or_else(|| {
                            let body =
                                crate::completion::source::helpers::extract_closure_body_expr_text(
                                    arg_text,
                                )?;
                            let resolved =
                                Self::resolve_closure_body_type(arg_text, body, None, ctx)?;
                            extract_array_position(&resolved, position)
                        });
                    if let Some(extracted) = extracted {
                        crate::type_engine::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            extracted,
                        );
                    }
                }
                TemplateBindingMode::CallableParamType(position) => {
                    // `@param Closure(T): void $cb` — extract the closure's
                    // parameter type annotation at the given position.
                    if let Some(param_type) = bind_callable_param_template(arg_text, position, ctx)
                    {
                        crate::type_engine::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            param_type,
                        );
                    }
                }
                TemplateBindingMode::ArrayElement => {
                    // `@param T[] $items` or `@param array<T> $items` —
                    // resolve individual array elements from array literals.
                    // For `[1, 2, 3]`, extract the first element `1` and
                    // resolve it to `int` so that `T = int`.
                    if arg_text.starts_with('[') && arg_text.ends_with(']') {
                        let inner = arg_text[1..arg_text.len() - 1].trim();
                        if !inner.is_empty() {
                            let first_elem =
                                crate::type_engine::types::conditional::split_text_args(inner);
                            if let Some(elem) = first_elem.first()
                                && let Some(resolved_type) =
                                    Self::resolve_arg_text_to_type(elem.trim(), ctx)
                            {
                                crate::type_engine::variable::rhs_resolution::insert_or_union(
                                    &mut subs,
                                    tpl_name.to_string(),
                                    resolved_type,
                                );
                            }
                        }
                    } else if let Some(resolved_type) =
                        Self::resolve_arg_text_to_type(arg_text, ctx)
                    {
                        // Extract the element type from array-like types
                        // so we bind T to the element, not the whole array.
                        if let Some(elem_type) =
                            crate::type_engine::variable::rhs_resolution::array_element_binding(
                                resolved_type,
                            )
                        {
                            crate::type_engine::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                elem_type,
                            );
                        }
                    }
                }
                TemplateBindingMode::ClassStringInner => {
                    if let Some(binding) =
                        crate::type_engine::variable::rhs_resolution::class_string_inner_binding(
                            arg_text, ctx,
                        )
                    {
                        crate::type_engine::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            binding,
                        );
                    }
                }
            }
        }

        subs
    }

    /// When a `GenericWrapper` extraction fails and the argument is a
    /// closure, try to infer the template param from the closure's
    /// return type (explicit annotation or yield inference).
    ///
    /// This handles union param types like
    /// `iterable<TKey, TValue>|(Closure(): Generator<TKey, TValue, mixed, void>)`
    /// where the classifier picked `GenericWrapper("iterable", pos)` but
    /// the arg is actually a closure.  We look for a `Callable` variant
    /// in the param hint union whose return type contains the template
    /// param, infer the closure's return type (via annotation or yields),
    /// and extract the generic arg at `tpl_position`.
    pub(crate) fn try_closure_return_type_for_template(
        arg_text: &str,
        tpl_name: &str,
        tpl_position: usize,
        param_hint: Option<&PhpType>,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        // Check that the param hint union contains a Callable variant
        // whose return type is a Generic containing the template param.
        let callable_return_type =
            Self::find_callable_return_generic_in_hint(param_hint?, tpl_name)?;

        let trimmed = arg_text.trim();

        // Infer the closure's effective return type.
        let closure_ret = if let Some(ret) = Self::infer_closure_return_type(arg_text, ctx) {
            ret
        } else {
            // Variable/chain argument like `$closure`: resolve the argument
            // type and, when it is a typed Closure(), unwrap its return type.
            let resolved = Self::resolve_arg_text_to_type(trimmed, ctx)?;
            match resolved.callable_return_type() {
                Some(ret) if resolved.is_closure() => ret.clone(),
                _ => return None,
            }
        };

        // Match the inferred return type against the expected generic
        // shape.  E.g., if callable returns `Generator<TKey, TValue, ...>`
        // and we inferred `Generator<int, string, mixed, mixed>`, extract
        // the arg at tpl_position.
        if let (TypeKind::Generic(expected), TypeKind::Generic(inferred)) =
            (callable_return_type.kind(), closure_ret.kind())
        {
            let exp_short = crate::util::short_name(&expected.name);
            let inf_short = crate::util::short_name(&inferred.name);
            if exp_short.eq_ignore_ascii_case(inf_short) {
                return inferred.args.get(tpl_position).cloned();
            }
        }

        // If the return type itself IS the template param (Closure(): T),
        // return the whole inferred type.
        if callable_return_type.is_named(tpl_name) {
            return Some(closure_ret);
        }

        None
    }

    /// Search a (possibly union) param type for a `Callable` variant whose
    /// return type is a Generic containing the given template param name.
    /// Returns that Generic return type if found.
    fn find_callable_return_generic_in_hint(hint: &PhpType, tpl_name: &str) -> Option<PhpType> {
        match hint.kind() {
            TypeKind::Union(members) => {
                for m in members {
                    if let Some(found) = Self::find_callable_return_generic_in_hint(m, tpl_name) {
                        return Some(found);
                    }
                }
                None
            }
            TypeKind::Nullable(inner) => {
                Self::find_callable_return_generic_in_hint(inner, tpl_name)
            }
            TypeKind::Callable(c) => {
                if let Some(rt) = &c.return_type
                    && crate::type_engine::variable::rhs_resolution::type_contains_name(
                        rt, tpl_name,
                    )
                {
                    return Some(rt.clone());
                }
                None
            }
            _ => None,
        }
    }

    /// Resolve an argument text string to a type name.
    ///
    /// Handles common patterns:
    /// - `ClassName::class` → `ClassName`
    /// - `new ClassName(…)` → `ClassName`
    /// - `$this` / `self` / `static` → current class name
    /// - `$this->prop` → property type
    /// - `$var` → variable type via assignment scanning
    /// - `"hello"` / `'world'` → `string`
    /// - `42` / `-1` → `int`
    /// - `3.14` → `float`
    /// - `true` / `false` → `bool`
    /// - `null` → `null`
    /// - `[…]` → `array`
    /// - `EnumClass::Case` → `EnumClass`
    /// - `ClassName::CONSTANT` → constant's declared type
    pub(crate) fn resolve_arg_text_to_type(
        arg_text: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let trimmed = arg_text.trim();

        // ── Literal values ──────────────────────────────────────
        if let Some(ty) = resolve_literal_type(trimmed) {
            return Some(ty);
        }

        // ── Casts ───────────────────────────────────────────────
        // `(string) $customer->id` is a `string` whatever the property
        // resolves to, so the cast answers before the operand is read.
        if let Some(ty) = resolve_cast_type(trimmed) {
            return Some(ty);
        }

        // ClassName::class → class-string<ClassName>
        //
        // The magic `::class` constant yields the fully-qualified class
        // name as a `class-string<T>`, mirroring the general expression
        // resolver (`resolve_rhs_property_access`).  Keeping the wrapper
        // here means a template param bound directly from a `::class`
        // argument (`@param T $x`) infers `class-string<T>` rather than
        // the bare class, matching the argument's actual type.  The
        // `class-string<T>` unwrapping paths (ClassStringInner and the
        // class-string generic wrapper) strip the wrapper back off when
        // they need the bare class.
        if let Some(name) = trimmed.strip_suffix("::class")
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
        {
            // self::class / static::class / parent::class resolve relative
            // to the class at the call site.
            let class_named = if is_self_or_static(name) {
                ctx.current_class.map(|c| PhpType::named(c.fqn()))
            } else if name.eq_ignore_ascii_case("parent") {
                ctx.current_class
                    .and_then(|c| c.parent_class.as_ref())
                    .map(|p| PhpType::named(atom(p.as_ref())))
            } else {
                let resolved_name = if let Some(cls) = (ctx.class_loader)(name) {
                    cls.fqn().to_string()
                } else {
                    name.to_string()
                };
                Some(PhpType::named(atom(&resolved_name)))
            };
            return class_named.map(|n| PhpType::class_string(Some(n)));
        }

        // Global constant access: `PHP_VERSION`, `PHP_EOL`, etc.
        //
        // A bare identifier that isn't a keyword, a `::class`/enum/const
        // access (handled above and below), or any other special form is a
        // global constant reference.  Ask the attached `Backend` (the same
        // source `VarResolutionCtx`'s constant loader draws from) and infer
        // the type from its value, mirroring the
        // `Expression::ConstantAccess` branch the AST-based RHS resolver
        // already has for a plain `$x = PHP_EOL;` assignment.  This path
        // takes expression *text*, with no offset to resolve a namespaced
        // name against, so only the name as written is tried.
        if !trimmed.is_empty()
            && !trimmed.starts_with('$')
            && !trimmed.contains("::")
            && !trimmed.contains("->")
            && !trimmed.contains('(')
            && !trimmed.contains('[')
            && trimmed
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
            && !is_self_or_static(trimmed)
            && !trimmed.eq_ignore_ascii_case("parent")
            && let Some(backend) = ctx.backend
            && let Some(Some(value)) = backend.lookup_global_constant(trimmed)
            && let Some(ty) =
                crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(&value)
                    .or_else(|| super::folded_global_constant_type(trimmed, &value, ctx))
        {
            return Some(ty);
        }

        // When the expression contains a `->` chain (e.g.
        // `Country::DK->value`, `new Decimal($x)->toFixed(2)`),
        // skip the static-access and new-expression shortcuts —
        // they would match the prefix and ignore the chain.
        // Let `resolve_expression_to_type` handle the full chain.
        let has_arrow_chain = trimmed.contains("->");

        // ClassName::Member — enum cases and class constants.
        // Enum cases resolve to the enum type; class constants
        // resolve to the constant's declared type hint.
        if !has_arrow_chain && let Some(ty) = resolve_static_access_type(trimmed, ctx) {
            return Some(ty);
        }

        // new ClassName(…) → ClassName
        if !has_arrow_chain
            && let Some(class_name) =
                crate::completion::source::helpers::extract_new_expression_class(trimmed)
        {
            let resolved_name = if let Some(cls) = (ctx.class_loader)(&class_name) {
                cls.fqn().to_string()
            } else {
                class_name
            };
            return Some(PhpType::named(atom(&resolved_name)));
        }

        // $this / self / static → current class (or preserve the keyword when asked)
        if is_self_or_static(trimmed) {
            return ctx.current_class.map(|c| {
                if ctx.preserve_static {
                    match trimmed {
                        "static" => PhpType::static_type(c.fqn()),
                        "$this" => PhpType::this_type(c.fqn()),
                        _ => PhpType::named(c.fqn()),
                    }
                } else {
                    PhpType::named(atom(c.name.as_ref()))
                }
            });
        }

        // When preserve_static is set, try resolving method chains by
        // looking up the last method's declared return type directly.
        // This preserves $this/static and generics that the general
        // expression resolver would flatten to a bare class name.
        if ctx.preserve_static
            && trimmed.contains("->")
            && let Some(ty) = resolve_chain_declared_return(trimmed, ctx)
        {
            return Some(ty);
        }

        // Operators whose result is decided from their operands rather
        // than from source-text shape alone (`$a . $b`, `$body ?: ''`).
        // Checked before the general fallback because `SubjectExpr::parse`
        // has no notion of these operators and would otherwise misread the
        // whole expression as a single bare variable or class name.
        if let Some(ty) = resolve_operator_type(trimmed, ctx) {
            return Some(ty);
        }

        // General expression fallback: parse the argument text as a
        // SubjectExpr and try to resolve it to a type.  This handles
        // $var, $var->prop, $this->prop, $var->method(), method
        // chains, and any other expression pattern.
        if let Some(ty) = resolve_expression_to_type(trimmed, ctx) {
            return Some(ty);
        }

        // A call whose return type names no class (`$review->getRating()`
        // returning `int`) leaves the general resolver empty, because it
        // reports classes.  Read the call's return type from the same
        // resolution path so a template can still bind from it.
        if let Some(ty) = resolve_call_return_hint(trimmed, ctx) {
            return Some(ty);
        }

        // The general resolver only reports class-backed results, so a
        // property or variable holding a non-class type (`array<string,
        // Leaf>`) comes back empty.  Read the declared type directly so
        // template params can still bind from it.
        crate::type_engine::variable::rhs_resolution::resolve_arg_variable_raw_type(trimmed, ctx)
    }

    /// Infer a closure/arrow-function argument's effective return type.
    ///
    /// Three sources are tried in turn: an explicit `: ReturnType`
    /// annotation, generator `yield` inference, and finally the body
    /// expression resolved through the shared type resolver (an arrow
    /// `fn() => EXPR`, or the first `return EXPR;` of a full closure body).
    /// The body-resolution fallback lets template params bind from
    /// unannotated closures like `Cache::remember($k, $ttl, fn() => new
    /// Order())`.
    ///
    /// Returns `None` when the text is not a closure literal or nothing can
    /// be inferred.
    pub(crate) fn infer_closure_return_type(
        arg_text: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        crate::completion::source::helpers::extract_closure_return_type_from_text(arg_text)
            // A `: ReturnType` annotation is raw source text, so its class
            // names are still spelled as the file writes them (`Support\Pen`
            // behind a `use App\Support;`).  A template bound from it is
            // compared against types that arrived fully qualified, so the
            // spelling has to be canonicalised before it is bound.
            .map(|ty| crate::util::resolve_php_type_names(&ty, ctx.class_loader))
            .or_else(|| {
                crate::completion::source::helpers::infer_generator_type_from_closure_yields(
                    arg_text,
                )
            })
            .or_else(|| {
                let body =
                    crate::completion::source::helpers::extract_closure_body_expr_text(arg_text)?;
                // A body that resolves to `mixed` says nothing about the
                // template, and binding it hides the template's own bound
                // (`@template TNewKey of array-key`), which is strictly more
                // informative.  Leave the template unbound instead.
                Self::resolve_closure_body_type(arg_text, body, None, ctx)
                    .filter(|ty| !ty.is_mixed())
            })
    }

    /// Infer a closure/arrow-function argument's return type from its
    /// body, with its first parameter seeded to `param_type`.
    ///
    /// The call site sometimes knows what the callback's first
    /// parameter receives even though the callback leaves it untyped:
    /// `array_map($cb, $users)` hands `$cb` a `User`.  Unlike
    /// [`infer_closure_return_type`](Self::infer_closure_return_type)
    /// this skips the `: ReturnType` annotation, which the caller has
    /// already consulted.
    pub(crate) fn infer_closure_return_type_from_body(
        arg_text: &str,
        param_type: &PhpType,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let body = crate::completion::source::helpers::extract_closure_body_expr_text(arg_text)?;
        Self::resolve_closure_body_type(arg_text, body, Some(param_type), ctx)
    }

    /// Resolve an unannotated closure's body expression to a type,
    /// seeding the closure's own typed parameters into variable
    /// resolution.
    ///
    /// A body expression rooted at a closure parameter (e.g.
    /// `fn(Decimal $carry, $op) => $carry->add(...)`) cannot resolve
    /// through outer-scope assignment scanning because the parameter is
    /// declared in the closure's own signature.  This injects a
    /// `scope_var_resolver` that answers parameter lookups from the
    /// declared type hints and delegates everything else to the
    /// resolution the body would otherwise get (the outer scope
    /// resolver when present, assignment scanning otherwise).
    ///
    /// `first_param_seed` supplies the type of the first parameter when
    /// the closure declares none and the call site knows what it
    /// receives (see
    /// [`infer_closure_return_type_from_body`](Self::infer_closure_return_type_from_body)).
    fn resolve_closure_body_type(
        closure_text: &str,
        body: &str,
        first_param_seed: Option<&PhpType>,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let typed_params: Vec<(String, PhpType)> =
            crate::completion::source::helpers::extract_closure_params_from_text(closure_text)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .filter_map(|(index, (name, ty))| match ty {
                    Some(t) => Some((name, t)),
                    None if index == 0 => first_param_seed.map(|t| (name, t.clone())),
                    None => None,
                })
                .collect();
        if typed_params.is_empty() {
            return Self::resolve_arg_text_to_type(body, ctx);
        }

        // Pre-resolve each typed parameter to its classes so the
        // injected resolver is a cheap map lookup.
        let owning_class_name = ctx.current_class.map(|c| c.name.as_str()).unwrap_or("");
        let seed_one = |ty: PhpType| -> Vec<ResolvedType> {
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &ty,
                owning_class_name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if classes.is_empty() {
                vec![ResolvedType::from_type_string(ty)]
            } else {
                ResolvedType::from_classes_with_hint(classes, ty)
            }
        };
        let param_types: HashMap<String, Vec<ResolvedType>> = typed_params
            .into_iter()
            .map(|(name, ty)| {
                // The parameter hint is raw source text, so it carries the
                // file's own spelling of the class name; canonicalise it so
                // the seeded type matches one resolved any other way.
                let ty = crate::util::resolve_php_type_names(&ty, ctx.class_loader);
                // Each alternative of a union is seeded on its own so it
                // keeps its own generic arguments. Two instantiations of the
                // same class (`Builder<A>|Builder<B>`) resolve to one class,
                // and a single entry could only carry the whole union as its
                // type string — leaving a `@return T` on the class with no
                // instantiation to substitute from.
                let resolved = match ty.kind() {
                    TypeKind::Union(members) => {
                        members.iter().cloned().flat_map(&seed_one).collect()
                    }
                    _ => seed_one(ty),
                };
                (name, resolved)
            })
            .collect();

        let outer_resolver = ctx.scope_var_resolver;
        let param_aware_resolver = move |name: &str| -> Vec<ResolvedType> {
            if let Some(types) = param_types.get(name) {
                return types.clone();
            }
            match outer_resolver {
                Some(outer) => outer(name),
                // No outer scope resolver: replicate the assignment-scan
                // fallback the body resolution would otherwise take for
                // this variable (see `resolve_variable_fallback`).
                None => {
                    let dummy_class;
                    let effective_class = match ctx.current_class {
                        Some(cc) => cc,
                        None => {
                            dummy_class = crate::class_lookup::class_context_placeholder(
                                ctx.content,
                                ctx.cursor_offset,
                            );
                            &dummy_class
                        }
                    };
                    crate::type_engine::variable::resolution::resolve_variable_types(
                        name,
                        effective_class,
                        ctx.all_classes,
                        ctx.content,
                        ctx.cursor_offset,
                        ctx.class_loader,
                        ctx.backend,
                        Loaders::with_function(ctx.function_loader),
                    )
                }
            }
        };

        let param_ctx = ResolutionCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            laravel_macro_this_resolver: ctx.laravel_macro_this_resolver,
            resolved_class_cache: ctx.resolved_class_cache,
            function_loader: ctx.function_loader,
            scope_var_resolver: Some(&param_aware_resolver),
            is_in_static_method: ctx.is_in_static_method,
            preserve_static: ctx.preserve_static,
        };
        Self::resolve_arg_text_to_type(body, &param_ctx)
    }
}

/// Build the full template substitution map for a method call: class-level
/// substitutions from the receiver's own generic arguments, method-level
/// substitutions bound from the call's arguments, and `@psalm-if-this-is`
/// substitutions inferred from the receiver's concrete type.
///
/// Shared by call-site return-type resolution
/// ([`crate::type_engine::variable::rhs_resolution::calls`]) and
/// `@psalm-this-out` receiver mutation (the forward walker) — both need
/// the same three-layer substitution map, just applied to different
/// target types (the method's return type vs. its self-out type).
pub(crate) fn build_call_template_subs(
    owner: &ClassInfo,
    method_name: &str,
    arg_texts: &[&str],
    receiver_type: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let class_level_subs: HashMap<String, PhpType> = receiver_type
        .map(|ty| {
            if ty.is_self_like()
                || matches!(ty.kind(), TypeKind::Generic(g) if g.args.iter().any(|a| a.is_self_like()))
            {
                return HashMap::new();
            }

            let mut values: HashMap<String, PhpType> = owner
                .template_param_defaults
                .iter()
                .map(|(name, default)| (name.to_string(), default.clone()))
                .collect();
            if let TypeKind::Generic(g) = ty.kind()
                && !g.args.is_empty()
                && !owner.template_params.is_empty()
            {
                values.extend(
                    owner
                        .template_params
                        .iter()
                        .zip(g.args.iter())
                        .map(|(name, ty)| (name.to_string(), ty.clone())),
                );
            }
            values
        })
        .unwrap_or_default();

    let method_template_subs =
        Backend::build_method_template_subs(owner, method_name, arg_texts, ctx);

    let if_this_is_subs: HashMap<String, PhpType> = owner
        .get_method_ci(method_name)
        .and_then(|m| m.if_this_is.as_ref())
        .and_then(|pattern| {
            let method = owner.get_method_ci(method_name)?;
            Some(
                crate::type_engine::variable::rhs_resolution::infer_if_this_is_subs(
                    pattern,
                    receiver_type?,
                    &method.template_params,
                    &method.template_param_bounds,
                ),
            )
        })
        .unwrap_or_default();

    let mut template_subs = class_level_subs;
    template_subs.extend(method_template_subs);
    template_subs.extend(if_this_is_subs);
    template_subs
}

/// Parameter names (`$`-prefixed) that are the *exclusive* binding site
/// for a `@template` parameter at this call site.
///
/// A template bound from exactly one argument has no independent
/// signal to check that argument against: the substituted type came from
/// resolving that same argument, so any diagnostic that compares the
/// argument to it again is comparing the argument against itself through
/// two potentially-diverging resolution paths. For example, PHPUnit's
/// `assertSame(ExpectedType $expected, mixed $actual)` binds
/// `ExpectedType` only from `$expected`, so checking `$expected` against
/// `ExpectedType` is circular and can never legitimately fail.
///
/// What counts is the binding sites the *caller filled*, not the ones the
/// signature declares. Laravel's `travelTo` names `TDate` in both `$date`
/// and the optional `$callback`'s callable signature; a call that passes
/// only a date still binds `TDate` from that one argument, so checking it
/// is just as circular as if `$callback` did not exist. Omitted
/// parameters are dropped here for that reason.
///
/// A template two arguments both bind is not covered: those disagree with
/// each other rather than with themselves, which is a real check.
pub(crate) fn self_bound_template_params(
    bindings: &[(Atom, Atom)],
    parameters: &[ParameterInfo],
    arg_texts: &[&str],
) -> AtomSet {
    let bound = crate::call_args::bind_text_args_to_params(parameters, arg_texts);
    let was_passed = |param_name: &Atom| {
        parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
            .is_some_and(|idx| bound.get(idx).is_some_and(Option::is_some))
    };
    let filled: Vec<&(Atom, Atom)> = bindings
        .iter()
        .filter(|(_, param_name)| was_passed(param_name))
        .collect();

    let mut result = AtomSet::default();
    for (tpl_name, param_name) in &filled {
        if filled.iter().filter(|(t, _)| t == tpl_name).count() == 1 {
            result.insert(*param_name);
        }
    }
    result
}

/// Resolve an array literal argument's first element to a type.
///
/// `resolve_arg_text_to_type("[1, 2, 3]")` collapses the whole literal to
/// a bare `array` with no element type, so callers that need the element
/// type itself (binding a template through an array-like wrapper) must
/// unwrap the literal and resolve the first element directly instead.
///
/// Returns `None` when `arg_text` is not a `[...]` literal, or the literal
/// is empty.
fn first_array_literal_element_type(arg_text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))?
        .trim();
    if inner.is_empty() {
        return None;
    }
    let elems = crate::type_engine::types::conditional::split_text_args(inner);
    let elem = elems.first()?;
    Backend::resolve_arg_text_to_type(elem.trim(), ctx)
}

/// Build an `array{key: type, ...}` shape from an array literal argument's
/// own keys and values.
///
/// `resolve_arg_text_to_type("['debug' => false]")` collapses the whole
/// literal to the bare `array` keyword, which is enough for most callers
/// but erases the literal's own keys. A template bound directly to the
/// whole argument (no wrapping hint to unify against) needs those keys
/// preserved so `key-of<T>`/`value-of<T>` on the bound template can still
/// project them out.
///
/// Only keyed entries are recorded — a mixed literal's positional elements
/// are dropped, matching the AST-based array literal inference in
/// `raw_type_inference.rs`. Returns `None` when `arg_text` is not a
/// `[...]`/`array(...)` literal, or none of its entries have a literal
/// string/int key.
pub(crate) fn array_literal_shape_type(arg_text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .or_else(|| {
            trimmed
                .strip_prefix("array(")
                .and_then(|s| s.strip_suffix(')'))
        })?
        .trim();
    if inner.is_empty() {
        return None;
    }

    let mut entries = Vec::new();
    // The key PHP gives the next element written without one: one past the
    // largest integer key so far.  Unknown once a key we cannot read (or a
    // spread) could have been an integer.
    let mut next_index: Option<i64> = Some(0);
    for elem in crate::type_engine::types::conditional::split_text_args(inner) {
        let elem = elem.trim();
        if elem.is_empty() {
            continue;
        }
        let (key, value_text) = match elem.find("=>") {
            Some(arrow_pos) => {
                let key = literal_array_key_text(elem[..arrow_pos].trim());
                match key.as_deref().map(str::parse::<i64>) {
                    Some(Ok(int_key)) => {
                        next_index = next_index.map(|next| next.max(int_key.saturating_add(1)));
                    }
                    Some(Err(_)) => {}
                    None => next_index = None,
                }
                (key, elem[arrow_pos + 2..].trim())
            }
            None if elem.starts_with("...") => {
                next_index = None;
                (None, elem)
            }
            None => {
                let key = next_index.map(|index| index.to_string());
                next_index = next_index.map(|index| index.saturating_add(1));
                (key, elem)
            }
        };
        let Some(key) = key else {
            continue;
        };
        // `resolve_arg_text_to_type` widens a scalar literal to its base
        // type (`1` → `int`), which would leave `value-of<T>` over the
        // bound shape with the scalar rather than the literal the caller
        // wrote. Keep int/float/string literals precise; everything else
        // (`true`/`false`, `null`, variables, calls) resolves as before.
        let value_type =
            crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
                value_text,
            )
            .filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
            .or_else(|| Backend::resolve_arg_text_to_type(value_text, ctx))
            .unwrap_or_else(PhpType::mixed);
        entries.push(crate::php_type::ShapeEntry {
            key: Some(key),
            value_type,
            optional: false,
        });
    }

    (!entries.is_empty()).then(|| PhpType::array_shape(entries))
}

/// The literal an argument binds a template to when the template's own bound
/// is a type operator — `@template K of key-of<TABLE>`, `@template V of
/// value-of<TABLE>`, `@template E of TABLE[K]`.
///
/// `resolve_arg_text_to_type` widens a scalar literal to its base type, which
/// is what nearly every binding wants and exactly wrong here: the operator can
/// only be evaluated against the *specific* key or value the caller wrote, so
/// `'immutable'` has to stay `'immutable'` rather than becoming `string`. The
/// quotes are kept because that is the spelling `evaluate_index_access`
/// matches shape keys against.
///
/// Returns `None` when the bound is not a type operator or the argument is not
/// a scalar literal, leaving the ordinary binding modes to resolve it.
pub(crate) fn type_operator_bound_literal(
    bound: Option<&PhpType>,
    arg_text: &str,
) -> Option<PhpType> {
    let bound = bound?;
    if !matches!(
        bound.kind(),
        TypeKind::KeyOf(_) | TypeKind::ValueOf(_) | TypeKind::IndexAccess(..)
    ) {
        return None;
    }
    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(arg_text.trim())
        .filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
}

/// The array shape a constant read through a type operator describes.
///
/// `key-of<ID_TABLE>` and `ID_TABLE[K]` name an operand as concrete as an
/// inline `array{…}`, but the docblock parser only ever sees the name — it
/// cannot read the constant behind it. This does, from the constant's own
/// initializer text, for a global constant (`ID_TABLE`) and for the
/// `Class::CONST` spelling alike.
///
/// Returns `None` when the name is not a constant we can reach, when its
/// value is not an array literal, or when none of its keys are literal — in
/// each case the operator stays unevaluated and widens to its bound, which
/// is the honest reading of an operand nobody can read.
pub(crate) fn constant_operand_shape(name: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let value = match name.rsplit_once("::") {
        Some((class_part, const_name)) => {
            // `class_part` is a source-level reference, so an unqualified
            // name must resolve against the declaring class's namespace
            // before falling back to the global scope — otherwise a file
            // with several braced `namespace` blocks that each declare the
            // same short class name always picks the first one, regardless
            // of which block actually declared the operand.
            let class_name =
                crate::class_lookup::resolve_class_keyword(class_part, ctx.current_class)
                    .unwrap_or_else(|| {
                        let ns = ctx.current_class.and_then(|c| c.file_namespace.as_deref());
                        crate::util::resolve_source_class_name(class_part, ns, ctx.class_loader)
                    });
            let class = crate::class_lookup::find_class_by_name(ctx.all_classes, &class_name)
                .cloned()
                .or_else(|| (ctx.class_loader)(&class_name))?;
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &class,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            merged.get_constant(const_name)?.value.clone()?
        }
        // The name may arrive in either spelling: some paths hand over what
        // `resolve_names` already qualified against the file's namespace,
        // since almost every bare name in a type position is a class, while
        // the ones that resolve names through the class loader alone leave a
        // constant bare.
        None => resolve_operand_constant_value(name, ctx)?,
    };
    array_literal_shape_type(&value, ctx)
}

/// The initializer text of the constant a type operand names, or `None`
/// when no spelling of it is indexed.
///
/// A constant is indexed under its fully-qualified name, so the enclosing
/// namespace is tried first for a bare operand, then the name as written,
/// then the global constant of that short name the way PHP itself falls
/// back, and finally the file's `use const` imports.
///
/// Only the indexed lookup is used, never the one that ends by parsing
/// every autoload file: an operand is as often a template parameter as a
/// constant, and a miss must not charge the whole autoload set.
fn resolve_operand_constant_value(name: &str, ctx: &ResolutionCtx<'_>) -> Option<String> {
    let backend = ctx.backend?;
    // An absolute name names exactly one constant, with no namespace or
    // import in play.
    if let Some(absolute) = name.strip_prefix('\\') {
        return backend.lookup_indexed_global_constant(absolute);
    }
    let short = crate::util::short_name(name);
    if short == name
        && let Some(value) = ctx
            .current_class
            .and_then(|c| c.file_namespace.as_ref())
            .and_then(|ns| backend.lookup_indexed_global_constant(&format!("{ns}\\{name}")))
    {
        return Some(value);
    }
    if let Some(value) = backend.lookup_indexed_global_constant(name) {
        return Some(value);
    }
    if short != name
        && let Some(value) = backend.lookup_indexed_global_constant(short)
    {
        return Some(value);
    }
    // Last: a `use const` import, which is the one spelling neither the
    // enclosing namespace nor the global scope accounts for.  The file's
    // import table is read from the source here because a type operand
    // carries no offset to resolve against, and it is read only once
    // every cheaper candidate has missed — a docblock naming an imported
    // constant is rare, and the walk runs over the already-parsed AST.
    let use_map = backend.parse_use_statements(ctx.content);

    let imported = match name.split_once('\\') {
        Some((first, rest)) => use_map.get(first).map(|fqn| format!("{fqn}\\{rest}")),
        None => use_map.get(name).cloned(),
    }?;
    backend.lookup_indexed_global_constant(&imported)
}

/// The substitution map that resolves every constant operand a set of types
/// reads through an unevaluated type operator.
///
/// Names in `skip` are left out: a surviving operator's operand is either a
/// constant or a template parameter, and looking a template parameter up as
/// a constant only wastes work.
fn constant_operand_subs<'t>(
    types: impl Iterator<Item = &'t PhpType>,
    skip: &[Atom],
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut operands = Vec::new();
    for ty in types {
        if ty.contains_unevaluated_operator() {
            ty.unevaluated_operator_operands(&mut operands);
        }
    }

    let mut subs: HashMap<String, PhpType> = HashMap::new();
    for operand in operands {
        if subs.contains_key(&operand) || skip.iter().any(|p| p.as_str() == operand) {
            continue;
        }
        if let Some(shape) = constant_operand_shape(&operand, ctx) {
            subs.insert(operand, shape);
        }
    }
    subs
}

/// Evaluate the type operators a declared type reads through a constant.
///
/// `key-of<ID_TABLE>` on a parameter and `value-of<ID_TABLE>` on a return
/// describe a set of values as concrete as any written-out union, but the
/// docblock parser only saw a name it could not read and left the operator
/// standing. This reads the constant behind the name and finishes the
/// operator, so the type constrains its call sites like the union it is.
///
/// Every consumer of a declared parameter or return type goes through here,
/// not just the template path: a constant is no less readable from a
/// signature that declares no `@template`.
///
/// Returns `None` when the type has no unevaluated operator, when no operand
/// is a constant we can read, or when reading it changed nothing — in each
/// case the caller keeps the type it already has.
pub(crate) fn evaluate_constant_operands(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    if !ty.contains_unevaluated_operator() {
        return None;
    }
    let subs = constant_operand_subs(std::iter::once(ty), &[], ctx);
    if subs.is_empty() {
        return None;
    }
    let evaluated = ty.substitute(&subs);
    (evaluated != *ty).then_some(evaluated)
}

/// Recover a template parameter no argument names directly, from the
/// *bound* of a template that an argument did bind.
///
/// `usort`'s stub declares `@template T` and `@template TArray of array<T>`
/// with `@param TArray $array` / `@param callable(T, T): int $callback`.
/// Nothing at the call site binds `T`: the array argument binds `TArray`,
/// and the callback is the very thing whose parameters `T` is meant to
/// type. Unifying `TArray`'s bound (`array<T>`) against what `TArray` was
/// bound to (`list<Error>`) recovers `T = Error`, which is what turns an
/// untyped `usort($errors, fn ($a, $b) => …)` callback into a typed one.
///
/// The bound may also name a *supertype* of what the other template was
/// bound to rather than its own shape. `CollectedDataNode::get()` declares
/// `@template TCollector of Collector<Node, TValue>` with
/// `@param class-string<TCollector>`, so `TValue` is whatever the collector
/// class's own `@implements Collector<…, …>` says it is — read off the
/// class's ancestry rather than off the argument's shape.
///
/// Runs before the fill-in below, so it only ever reads bindings that came
/// from real arguments, never a bound standing in for a missing one.
fn propagate_bound_template_bindings(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &crate::atom::AtomMap<PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    for tpl_name in template_params {
        if subs.contains_key(tpl_name.as_str()) {
            continue;
        }
        // Iterate the declared order rather than the bounds map so the
        // binding a template picks up does not depend on hash order.
        for other in template_params {
            if other == tpl_name {
                continue;
            }
            let Some(bound) = template_param_bounds.get(other) else {
                continue;
            };
            let Some(bound_to) = subs.get(other.as_str()) else {
                continue;
            };
            if let Some(recovered) = unify_template(bound, bound_to, tpl_name)
                .or_else(|| ancestor_bound_binding(bound, bound_to, tpl_name, ctx))
            {
                subs.insert(tpl_name.to_string(), recovered);
                break;
            }
        }
    }
}

/// Read `tpl_name` off the ancestry of what another template was bound to,
/// for a bound that names a generic supertype (`@template TCollector of
/// Collector<Node, TValue>`).
///
/// A `class-string<TCollector>` parameter binds the class itself, so the
/// generic arguments live on its `@extends`/`@implements` clauses, not on
/// the bound type it was matched against.
fn ancestor_bound_binding(
    bound: &PhpType,
    bound_to: &PhpType,
    tpl_name: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let TypeKind::Generic(g) = bound.kind() else {
        return None;
    };
    let position = g.args.iter().position(|a| a.is_named(tpl_name))?;
    let subject = bound_to.unwrap_class_string_inner().unwrap_or(bound_to);
    crate::type_engine::variable::rhs_resolution::extract_generic_arg_from_ancestor(
        subject, &g.name, position, ctx,
    )
}

/// Finish a template substitution map: bind the constants its types read
/// through a type operator, recover the templates only another template's
/// bound names, then fill in the template params no argument bound.
///
/// The halves exist so a raw name never leaks downstream. An unbound
/// template resolves to its declared upper bound (`@template T of Foo` →
/// `Foo`) or `mixed`, following PHPStan's `resolveToBounds()`. A constant
/// operand resolves to the array shape it names, which is what lets the
/// substitution every call site already runs finish the operator:
/// `key-of<TABLE>` becomes the table's own keys, and `TABLE[K]` picks out
/// the single value the argument bound `K` to.
///
/// The constant bindings are applied to the bounds as well, so a call that
/// leaves `K` unbound still reads `TABLE[key-of<TABLE>]` as the table's
/// value union rather than giving up on the operator.
pub(crate) fn finish_template_subs(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &crate::atom::AtomMap<PhpType>,
    return_type: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    propagate_bound_template_bindings(subs, template_params, template_param_bounds, ctx);

    let constant_subs = constant_operand_subs(
        return_type
            .into_iter()
            .chain(template_param_bounds.values()),
        template_params,
        ctx,
    );

    for tpl_name in template_params {
        subs.entry(tpl_name.to_string()).or_insert_with(|| {
            template_param_bounds
                .get(tpl_name)
                .map(|bound| bound.substitute(&constant_subs))
                .unwrap_or_else(PhpType::mixed)
        });
    }

    for (name, shape) in constant_subs {
        subs.entry(name).or_insert(shape);
    }
}

/// Extract the literal key text of an array literal's key expression
/// (`'debug'`, `"verbose"`, `42`), unquoting string keys. Returns `None`
/// for any key that is not a literal (e.g. a constant or variable), since
/// those cannot be projected into a shape's key set.
fn literal_array_key_text(key_text: &str) -> Option<String> {
    if let Some(unquoted) = crate::text_scan::unquote_php_string(key_text) {
        return Some(unquoted.to_string());
    }
    let numeric = key_text.strip_prefix('-').unwrap_or(key_text);
    (!numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit()))
        .then(|| key_text.to_string())
}

/// Bind the template a `@param callable(...): …` hint names in its return
/// type, from the closure argument written at the call site.
///
/// The closure's own return type comes from its annotation, its generator
/// yields, or (unannotated) its body.  Two things then stand between that
/// type and the template:
///
/// The annotation may be less specific than what the closure actually
/// returns.  `fn ($c): array => arrStr($c)` says only `array` while
/// `arrStr()` declares `array<int, string>`, so a hint that asks for
/// `array<TKey, TValue>` has nothing to take a key or a value from.  When
/// the declared return has structure to fill and the closure's own is the
/// opaque `array` keyword, the body is resolved instead.
///
/// And the template is rarely the whole return type: `array<TKey, TValue>`,
/// `list<TValue>`, and Laravel's `Collection<TKey, TValue>|array<TKey,
/// TValue>` each name it at a position inside a larger shape.  The inferred
/// type is matched against that shape so each template binds to its own
/// part rather than to the whole return type.
/// Bind a template parameter that a `@param Closure(T): void` hint names in
/// the callback's *parameter* list, reading the type off the closure
/// argument's own annotation at `position`.
///
/// The annotation is raw source text, so it carries the file's spelling of
/// the class rather than its FQCN (`Support\Pen` behind a `use App\Support;`).
/// A template bound from it is compared against — and unioned with — types
/// that arrived fully qualified, so the spelling is canonicalised here.
pub(crate) fn bind_callable_param_template(
    arg_text: &str,
    position: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let param_type = crate::completion::source::helpers::extract_closure_param_type_from_text(
        arg_text, position,
    )?;
    Some(crate::util::resolve_php_type_names(
        &param_type,
        ctx.class_loader,
    ))
}

pub(crate) fn bind_callable_return_template(
    arg_text: &str,
    param_hint: Option<&PhpType>,
    tpl_name: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let declared_ret = param_hint.and_then(|h| h.callable_return_type());
    let mut ret_type = Backend::infer_closure_return_type(arg_text, ctx);

    // `callable(): T` binds the whole return type, so a bare `array` is the
    // right answer there and there is no shape to decompose against.  Any
    // richer declared return has parts the bare keyword cannot fill.
    let has_shape =
        declared_ret.is_some_and(|d| !matches!(d.kind(), TypeKind::Named(n) if &**n == tpl_name));

    if has_shape
        && ret_type.as_ref().is_some_and(PhpType::is_bare_array)
        && let Some(body_type) =
            crate::completion::source::helpers::extract_closure_body_expr_text(arg_text)
                .and_then(|body| Backend::resolve_closure_body_type(arg_text, body, None, ctx))
                .filter(|ty| !ty.is_mixed() && !ty.is_bare_array())
    {
        ret_type = Some(body_type);
    }

    let ret_type = ret_type?;
    let bound = declared_ret.and_then(|declared| unify_template(declared, &ret_type, tpl_name));
    Some(bound.unwrap_or(ret_type))
}

/// Bind a template parameter by walking a parameter hint and an argument
/// type together.
///
/// Returns the argument's subtree at whichever position `tpl_name` occupies
/// in `param_hint`.  For `@param array<string, array<string, T>> $in` and an
/// argument typed `array<string, array<string, Leaf>>`, that is `Leaf` —
/// where positional extraction, which unwraps a single level, would bind the
/// whole inner array.
///
/// A union hint offers several shapes and the argument picks one: for
/// `@param iterable<array-key, T>|T $value` (Laravel's `Collection::wrap()`)
/// an `array<string>` argument binds `T` to `string` through the iterable
/// alternative, while a `string` argument binds `T` to `string` through the
/// bare one.  The bare alternative matches anything, so it is only used when
/// no other alternative fits.
///
/// Returns `None` when the hint does not name the template, or when the two
/// shapes disagree, leaving the caller's positional extraction to run.
fn unify_template(param_hint: &PhpType, arg_type: &PhpType, tpl_name: &str) -> Option<PhpType> {
    match param_hint.kind() {
        TypeKind::Named(name) if &**name == tpl_name => Some(arg_type.clone()),
        TypeKind::Union(members) => {
            let mut bare: Option<PhpType> = None;
            for member in members {
                if member.is_null() {
                    continue;
                }
                if member.is_named(tpl_name) {
                    bare = Some(arg_type.clone());
                    continue;
                }
                if let Some(unified) = unify_template(member, arg_type, tpl_name) {
                    return Some(unified);
                }
            }
            bare
        }
        TypeKind::Generic(hint) => {
            if let TypeKind::Generic(arg) = arg_type.kind()
                && arg.args.len() == hint.args.len()
            {
                return hint
                    .args
                    .iter()
                    .zip(arg.args.iter())
                    .find_map(|(h, a)| unify_template(h, a, tpl_name));
            }
            // Two container types whose arguments don't line up positionally
            // (`iterable<TKey, TValue>` against `list<string>`) still line up
            // key-to-key and value-to-value.
            if !crate::type_engine::variable::rhs_resolution::is_array_like_wrapper(&hint.name) {
                return None;
            }
            let key_match = (hint.args.len() >= 2)
                .then(|| arg_type.extract_key_type(false))
                .flatten()
                .and_then(|k| unify_template(&hint.args[0], k, tpl_name));
            key_match.or_else(|| {
                let value_hint = hint.args.last()?;
                // An untyped `array` still says "the argument is a container",
                // its elements are just unknown — `mixed`.  Without this the
                // hint does not match at all and a bare `T` alternative in a
                // union hint binds the array itself as the element type.
                let mixed = PhpType::mixed();
                let value = match arg_type.extract_value_type(false) {
                    Some(v) => v,
                    None if arg_type.is_bare_array() => &mixed,
                    None => return None,
                };
                unify_template(value_hint, value, tpl_name)
            })
        }
        TypeKind::Array(inner) => match arg_type.kind() {
            TypeKind::Array(arg_inner) => unify_template(inner, arg_inner, tpl_name),
            // `T[]` against `array<K, V>` / `list<V>`: the value type lines up.
            _ => arg_type
                .extract_value_type(false)
                .and_then(|v| unify_template(inner, v, tpl_name)),
        },
        TypeKind::Nullable(inner) => unify_template(inner, arg_type.unwrap_nullable(), tpl_name),
        _ => None,
    }
}

/// Whether a generic hint names `tpl_name` as one of its own arguments.
///
/// The flat case (`array<TKey, TValue>`) is the positional extractor's
/// business — it knows the key/value arity quirks — so structural
/// unification stays out of its way.
fn names_template_directly(hint: &PhpType, tpl_name: &str) -> bool {
    matches!(hint.kind(), TypeKind::Generic(g)
        if g.args.iter().any(|a| matches!(a.kind(), TypeKind::Named(n) if &**n == tpl_name)))
}

#[cfg(test)]
mod class_template_sub_tests {
    use std::sync::Arc;

    use super::build_call_template_subs;
    use crate::atom::atom;
    use crate::php_type::PhpType;
    use crate::type_engine::resolver::ResolutionCtx;
    use crate::types::ClassInfo;

    #[test]
    fn unresolved_self_like_receivers_do_not_apply_class_defaults() {
        let owner = ClassInfo {
            name: atom("PendingRequest"),
            template_params: vec![atom("TAsync")],
            template_param_defaults: [(atom("TAsync"), PhpType::parse("false"))]
                .into_iter()
                .collect(),
            ..ClassInfo::default()
        };
        let classes = Vec::new();
        let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
        let ctx = ResolutionCtx {
            current_class: None,
            all_classes: &classes,
            content: "",
            cursor_offset: 0,
            class_loader: &class_loader,
            backend: None,
            laravel_macro_this_resolver: None,
            resolved_class_cache: None,
            function_loader: None,
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: false,
        };

        assert!(
            build_call_template_subs(&owner, "send", &[], Some(&PhpType::parse("static")), &ctx,)
                .is_empty()
        );
        assert!(
            build_call_template_subs(
                &owner,
                "send",
                &[],
                Some(&PhpType::parse("PendingRequest<static>")),
                &ctx,
            )
            .is_empty()
        );
    }
}
