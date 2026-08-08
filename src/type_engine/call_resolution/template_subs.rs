/// Template substitution for method-level `@template` parameters: builds
/// a substitution map from call-site argument texts and resolves closure
/// return types against bound template parameters.
use std::collections::HashMap;

use crate::Backend;
use crate::atom::{Atom, AtomSet, atom};
use crate::class_lookup::is_self_or_static;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::variable::rhs_resolution::{
    TemplateBindingMode, classify_template_binding,
};
use crate::types::*;

use crate::type_engine::resolver::{Loaders, ResolutionCtx};

use super::return_types::{
    resolve_call_return_hint, resolve_chain_declared_return, resolve_expression_to_type,
    resolve_literal_type, resolve_static_access_type,
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

            let arg_text = match bound.get(param_idx).and_then(|o| o.as_deref()) {
                Some(text) => text,
                None => {
                    let default_value = method
                        .parameters
                        .get(param_idx)
                        .and_then(|p| p.default_value.as_deref());
                    match &binding_mode {
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
                    }
                }
            };

            // When the template param has a key-of bound (e.g.
            // `@template K as key-of<TData>`) and the argument is a
            // string literal, resolve K to the literal value so that
            // indexed access types like `TData[K]` can look up the
            // specific key in the array shape.
            if let Some(bound) = method.template_param_bounds.get(&atom(tpl_name))
                && matches!(bound.kind(), TypeKind::KeyOf(_))
            {
                let trimmed = arg_text.trim();
                let is_string_lit = (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                    || (trimmed.starts_with('"') && trimmed.ends_with('"'));
                if is_string_lit {
                    // Store as Literal with quotes so evaluate_index_access
                    // can strip them when matching against shape keys.
                    crate::type_engine::variable::rhs_resolution::insert_or_union(
                        &mut subs,
                        tpl_name.to_string(),
                        PhpType::literal_string_raw(trimmed.to_string()),
                    );
                    continue;
                }
            }

            match binding_mode {
                TemplateBindingMode::Direct => {
                    if let Some(resolved_type) = Self::resolve_arg_text_to_type(arg_text, ctx) {
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
                    // `@param callable(...): T $cb` — infer the closure's
                    // return type from its annotation, generator yields, or
                    // (for unannotated closures) its resolved body expression.
                    let ret_type = Self::infer_closure_return_type(arg_text, ctx);
                    if let Some(ret_type) = ret_type {
                        crate::type_engine::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            ret_type,
                        );
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
                        if let Some(elem_type) = resolved_type.extract_value_type(false) {
                            crate::type_engine::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                elem_type.clone(),
                            );
                        } else {
                            crate::type_engine::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                resolved_type,
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

        // ── Fill in unbound method-level template params ────────
        // Any template parameter that was not bound from call-site
        // arguments is replaced with its declared upper bound
        // (`@template T of Foo` → `Foo`) or `mixed`.  This follows
        // PHPStan's `resolveToBounds()` semantics and prevents raw
        // template names like `TReduceReturnType` from leaking into
        // parameter and return types.
        for tpl_name in &method.template_params {
            let tpl_key = tpl_name.to_string();
            subs.entry(tpl_key).or_insert_with(|| {
                method
                    .template_param_bounds
                    .get(tpl_name)
                    .cloned()
                    .unwrap_or_else(PhpType::mixed)
            });
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
        let param_types: HashMap<String, Vec<ResolvedType>> = typed_params
            .into_iter()
            .map(|(name, ty)| {
                let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    &ty,
                    owning_class_name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                let resolved = if classes.is_empty() {
                    vec![ResolvedType::from_type_string(ty)]
                } else {
                    ResolvedType::from_classes_with_hint(classes, ty)
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

/// Parameter names (`$`-prefixed) that are the *exclusive* binding site
/// for a `@template` parameter.
///
/// A template bound from exactly one parameter has no independent
/// signal to check that parameter's argument against: the substituted
/// type came from resolving that same argument, so any diagnostic that
/// compares the argument to it again is comparing the argument against
/// itself through two potentially-diverging resolution paths. For
/// example, PHPUnit's `assertSame(ExpectedType $expected, mixed
/// $actual)` binds `ExpectedType` only from `$expected`, so checking
/// `$expected` against `ExpectedType` is circular and can never
/// legitimately fail.
///
/// A template bound from more than one parameter (or from a parameter
/// plus a return-type appearance elsewhere) is not covered here — those
/// still carry a real independent check.
pub(crate) fn self_bound_template_params(bindings: &[(Atom, Atom)]) -> AtomSet {
    let mut result = AtomSet::default();
    for (tpl_name, param_name) in bindings {
        let is_sole_binding = bindings.iter().filter(|(t, _)| t == tpl_name).count() == 1;
        if is_sole_binding {
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
