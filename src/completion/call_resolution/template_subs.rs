/// Template substitution for method-level `@template` parameters: builds
/// a substitution map from call-site argument texts and resolves closure
/// return types against bound template parameters.
use std::collections::HashMap;

use crate::Backend;
use crate::atom::atom;
use crate::completion::variable::rhs_resolution::{TemplateBindingMode, classify_template_binding};
use crate::php_type::PhpType;
use crate::types::*;
use crate::util::is_self_or_static;

use crate::completion::resolver::{Loaders, ResolutionCtx};

use super::return_types::{
    resolve_chain_declared_return, resolve_expression_to_type, resolve_literal_type,
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

        let mut subs = HashMap::new();

        // Bind the raw source-order argument texts to parameters by PHP's
        // rules so a named argument (`id: Foo::class`) is routed to the
        // parameter it targets rather than its ordinal slot, and its `name:`
        // prefix is stripped off the value.
        let bound = crate::call_args::bind_text_args_to_params(&method.parameters, arg_texts);

        for (tpl_name, param_name) in &method.template_bindings {
            // Find the parameter index for this binding.
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

            // Get the corresponding argument text.
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
                && matches!(bound, PhpType::KeyOf(_))
            {
                let trimmed = arg_text.trim();
                let is_string_lit = (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                    || (trimmed.starts_with('"') && trimmed.ends_with('"'));
                if is_string_lit {
                    // Store as Literal with quotes so evaluate_index_access
                    // can strip them when matching against shape keys.
                    crate::completion::variable::rhs_resolution::insert_or_union(
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
                        crate::completion::variable::rhs_resolution::insert_or_union(
                            &mut subs,
                            tpl_name.to_string(),
                            resolved_type,
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
                        crate::completion::variable::rhs_resolution::insert_or_union(
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
                    if crate::completion::variable::rhs_resolution::is_array_like_wrapper(
                        wrapper_name,
                    ) {
                        // Array literal: `[1, 2, 3]` — resolve individual
                        // elements to infer the element type.
                        // `resolve_arg_text_to_type("[1, 2, 3]")` returns
                        // bare `array` (no generics), so we must unwrap the
                        // literal and resolve the first element directly.
                        if arg_text.starts_with('[') && arg_text.ends_with(']') {
                            let inner = arg_text[1..arg_text.len() - 1].trim();
                            if !inner.is_empty() {
                                let elems =
                                    crate::completion::types::conditional::split_text_args(inner);
                                if let Some(elem) = elems.first()
                                    && let Some(resolved_elem) =
                                        Self::resolve_arg_text_to_type(elem.trim(), ctx)
                                {
                                    crate::completion::variable::rhs_resolution::insert_or_union(
                                        &mut subs,
                                        tpl_name.to_string(),
                                        resolved_elem,
                                    );
                                }
                            }
                            continue;
                        }

                        // Variable or expression argument: resolve to a
                        // typed value and extract the positional generic
                        // argument (key or value type).
                        if let Some(resolved_type) = Self::resolve_arg_text_to_type(arg_text, ctx) {
                            let generic_arg_count = param_hint
                                .and_then(|h| match h {
                                    crate::php_type::PhpType::Generic(_, args) => Some(args.len()),
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
                                crate::completion::variable::rhs_resolution::insert_or_union(
                                    &mut subs,
                                    tpl_name.to_string(),
                                    concrete,
                                );
                            } else {
                                crate::completion::variable::rhs_resolution::insert_or_union(
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
                            crate::completion::variable::rhs_resolution::insert_or_union(
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
                            if let PhpType::Generic(name, args) = &resolved_type {
                                let short = crate::util::short_name(name);
                                let wrapper_short = crate::util::short_name(wrapper_name);
                                if short == wrapper_short {
                                    // When the param hint has fewer
                                    // generic args than the resolved
                                    // type (e.g. `Iterator<T>` vs
                                    // `Iterator<int, ASTClass>`), the
                                    // single param-hint arg represents
                                    // the value/last type.
                                    let param_generic_count = param_hint
                                        .and_then(|h| match h {
                                            PhpType::Generic(_, a) => Some(a.len()),
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
                                if let PhpType::Generic(_, concrete_args) = &resolved_type {
                                    merged
                                        .template_params
                                        .iter()
                                        .zip(concrete_args.iter())
                                        .map(|(name, ty)| (name.to_string(), ty.clone()))
                                        .collect()
                                } else {
                                    HashMap::new()
                                };

                            // Search implements_generics first, then
                            // extends_generics.
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
                                    .and_then(|h| match h {
                                        PhpType::Generic(_, a) => Some(a.len()),
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
                            crate::completion::variable::rhs_resolution::insert_or_union(
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
                            crate::completion::variable::rhs_resolution::insert_or_union(
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
                        crate::completion::variable::rhs_resolution::insert_or_union(
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
                        crate::completion::variable::rhs_resolution::insert_or_union(
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
                                crate::completion::types::conditional::split_text_args(inner);
                            if let Some(elem) = first_elem.first()
                                && let Some(resolved_type) =
                                    Self::resolve_arg_text_to_type(elem.trim(), ctx)
                            {
                                crate::completion::variable::rhs_resolution::insert_or_union(
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
                            crate::completion::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                elem_type.clone(),
                            );
                        } else {
                            crate::completion::variable::rhs_resolution::insert_or_union(
                                &mut subs,
                                tpl_name.to_string(),
                                resolved_type,
                            );
                        }
                    }
                }
                TemplateBindingMode::ClassStringInner => {
                    if let Some(binding) =
                        crate::completion::variable::rhs_resolution::class_string_inner_binding(
                            arg_text, ctx,
                        )
                    {
                        crate::completion::variable::rhs_resolution::insert_or_union(
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
        if let (
            PhpType::Generic(expected_name, _),
            PhpType::Generic(inferred_name, inferred_args),
        ) = (&callable_return_type, &closure_ret)
        {
            let exp_short = crate::util::short_name(expected_name);
            let inf_short = crate::util::short_name(inferred_name);
            if exp_short.eq_ignore_ascii_case(inf_short) {
                return inferred_args.get(tpl_position).cloned();
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
        match hint {
            PhpType::Union(members) => {
                for m in members {
                    if let Some(found) = Self::find_callable_return_generic_in_hint(m, tpl_name) {
                        return Some(found);
                    }
                }
                None
            }
            PhpType::Nullable(inner) => Self::find_callable_return_generic_in_hint(inner, tpl_name),
            PhpType::Callable { return_type, .. } => {
                if let Some(rt) = return_type
                    && crate::completion::variable::rhs_resolution::type_contains_name(rt, tpl_name)
                {
                    return Some(rt.as_ref().clone());
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
            let class_named =
                if name.eq_ignore_ascii_case("self") || name.eq_ignore_ascii_case("static") {
                    ctx.current_class
                        .map(|c| PhpType::Named(c.fqn().to_string()))
                } else if name.eq_ignore_ascii_case("parent") {
                    ctx.current_class
                        .and_then(|c| c.parent_class.as_ref())
                        .map(|p| PhpType::Named(p.to_string()))
                } else {
                    let resolved_name = if let Some(cls) = (ctx.class_loader)(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    };
                    Some(PhpType::Named(resolved_name))
                };
            return class_named.map(|n| PhpType::ClassString(Some(Box::new(n))));
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
            return Some(PhpType::Named(resolved_name));
        }

        // $this / self / static → current class (or preserve the keyword when asked)
        if is_self_or_static(trimmed) {
            return ctx.current_class.map(|c| {
                if ctx.preserve_static {
                    PhpType::Named(trimmed.to_string())
                } else {
                    PhpType::Named(c.name.to_string())
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

        None
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
                Self::resolve_closure_body_type(arg_text, body, ctx)
            })
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
    fn resolve_closure_body_type(
        closure_text: &str,
        body: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let typed_params: Vec<(String, PhpType)> =
            crate::completion::source::helpers::extract_closure_params_from_text(closure_text)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(name, ty)| ty.map(|t| (name, t)))
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
                let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
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
                            dummy_class = ClassInfo::default();
                            &dummy_class
                        }
                    };
                    crate::completion::variable::resolution::resolve_variable_types(
                        name,
                        effective_class,
                        ctx.all_classes,
                        ctx.content,
                        ctx.cursor_offset,
                        ctx.class_loader,
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
