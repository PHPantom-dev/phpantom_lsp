//! The callee-shape arms of [`Backend::resolve_call_return_types_on_receiver`].
//!
//! Each function here is one arm of that method's `match callee`, which
//! dispatches to them and does nothing else.

use super::*;
use crate::type_engine::call_resolution::{
    facade_concrete_owner, is_reflected_property_call, is_reflected_property_class,
    resolve_reflected_property_at_call, resolve_reflected_property_at_new,
};

impl Backend {
    /// `base->method(…)`: the instance method call arm of
    /// [`Backend::resolve_call_return_types_on_receiver`].
    pub(super) fn return_types_of_method_call(
        base: &SubjectExpr,
        method_name: &str,
        text_args: &str,
        receiver: Option<Vec<ResolvedType>>,
        ctx: &ResolutionCtx<'_>,
        mut return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        // Resolve the base expression preserving generic type
        // arguments (e.g. `Collection<Product>`) so class-level
        // template parameters can be substituted in the method's
        // return type.
        let lhs_resolved: Vec<ResolvedType> = receiver.unwrap_or_else(|| {
            crate::type_engine::resolver::resolve_target_classes_expr(base, AccessKind::Arrow, ctx)
        });

        // A property read through the Reflection API: the name
        // handed to `getProperty()` decides the type, so the
        // stub's `ReflectionProperty` / `mixed` return types are
        // as specific as an annotation can be.
        if is_reflected_property_call(method_name)
            && let Some(ty) = resolve_reflected_property_at_call(
                method_name,
                &split_text_args(text_args).to_vec(),
                &lhs_resolved,
                ctx,
            )
        {
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &ty,
                "",
                ctx.all_classes,
                ctx.class_loader,
            );
            if let Some(ref mut hint_out) = return_type_hint_out {
                **hint_out = Some(ty);
            }
            return classes;
        }

        // Guard-aware auth user model: a `user()` call on a
        // `Guard`/`Request` subtype resolves to the model
        // configured for the guard named at the call site
        // (`auth('admin')`, `Auth::guard('admin')`,
        // `$request->user('admin')`), falling back to the
        // default-guard model otherwise.
        if method_name == "user"
            && let Some(classes) = resolve_auth_user_at_call(base, text_args, &lhs_resolved, ctx)
        {
            return classes;
        }

        // Laravel factory count state: `create()`/`make()` build
        // a single model, or a collection of them when the chain
        // set a count (`factory(3)`, `count(3)`, `times(3)`).
        if let Some((classes, hint)) = crate::virtual_members::laravel::resolve_factory_count_return(
            base,
            method_name,
            &lhs_resolved,
            ctx,
        ) {
            if let Some(ref mut hint_out) = return_type_hint_out {
                **hint_out = Some(hint);
            }
            return classes;
        }

        // Laravel request input: `header('X', '')`, `query()`,
        // `file('photo')` and the rest all declare one union
        // covering every way of calling them, and the call's own
        // arguments say which of those ways this is.
        if let Some(ty) =
            resolve_request_accessor_at_call(method_name, text_args, &lhs_resolved, ctx)
        {
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &ty,
                "",
                ctx.all_classes,
                ctx.class_loader,
            );
            if let Some(ref mut hint_out) = return_type_hint_out {
                **hint_out = Some(ty);
            }
            return classes;
        }

        // Laravel validated input: `validated()`, `validate([…])`
        // and `safe()->only([…])` return the array shape the
        // validation rules in scope describe.  An array shape has
        // no class, so it travels as the type hint alone.
        if let Some(shape) =
            resolve_validated_shape_at_call(base, method_name, text_args, &lhs_resolved, ctx)
        {
            if let Some(ref mut hint_out) = return_type_hint_out {
                **hint_out = Some(shape);
            }
            return Vec::new();
        }

        // Capture the raw return type hint while we iterate
        // the owner classes below.  We grab it from the first
        // owner that has a matching method — before the return
        // type gets flattened into ClassInfo.
        let mut hint_captured = false;
        let mut results = Vec::new();

        for rt in &lhs_resolved {
            let owner = match &rt.class_info {
                Some(ci) => Arc::clone(ci),
                None => continue,
            };

            // Extract class-level generic type arguments from the
            // resolved type string (e.g. `Collection<Product>` →
            // `[Product]`) so we can substitute class-level
            // template parameters (e.g. `TItem → Product`).
            // Skip self-like args ($this, self, static) because
            // they refer to the caller's class context which is
            // not available here.
            let class_level_subs: HashMap<String, PhpType> = match &rt.type_string.kind() {
                TypeKind::Generic(g)
                    if !g.args.is_empty()
                        && !owner.template_params.is_empty()
                        && !g.args.iter().any(|a| a.is_self_like()) =>
                {
                    owner
                        .template_params
                        .iter()
                        .zip(g.args.iter())
                        .map(|(name, ty)| (name.to_string(), ty.clone()))
                        .collect()
                }
                _ => HashMap::new(),
            };

            let split_args = split_text_args(text_args);
            let arg_refs = split_args.to_vec();
            let method_subs = Self::build_method_template_subs(&owner, method_name, &arg_refs, ctx);

            // Merge class-level generic substitutions with
            // method-level template substitutions.  Class-level
            // subs map e.g. `TItem → Product`; method-level subs
            // map method @template params from call-site args.
            // Method-level subs take precedence (inserted last).
            let mut template_subs = class_level_subs;
            template_subs.extend(method_subs);

            let var_resolver = build_var_resolver(ctx);

            // Capture the return type hint from the first owner
            // that has the method.  Apply template substitutions
            // so that generic return types like `T` are resolved
            // to their concrete types (e.g. `Product`).  Without
            // this, callers that use the hint for downstream
            // template binding would see unsubstituted params.
            if !hint_captured && let Some(ref mut hint_out) = return_type_hint_out {
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    &owner,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                if let Some(m) = merged.get_method_ci(method_name) {
                    // Try the conditional return type first, exactly
                    // as `resolve_method_return_types_with_args`
                    // does for the classes below — a conditional
                    // return type (e.g. `mock()`'s `(TInstance
                    // is class-string<T> ? T&MockInterface :
                    // MockInterface)`) often carries no plain
                    // `return_type` at all, so reading only
                    // `return_type` here would leave the hint
                    // empty even though the classes resolve fine,
                    // silently dropping the winning branch's shape
                    // (e.g. an intersection) from the hint.
                    let substituted = resolve_conditional_return_hint(
                        m,
                        text_args,
                        Some(&var_resolver),
                        &template_subs,
                        ctx.current_class.map(|c| c.name.as_str()),
                        merged.fqn().as_str(),
                        ctx.class_loader,
                    )
                    .or_else(|| {
                        m.return_type.as_ref().map(|ret| {
                            if !template_subs.is_empty() {
                                ret.substitute(&template_subs)
                            } else {
                                ret.clone()
                            }
                        })
                    });
                    if let Some(substituted) = substituted {
                        // Collapse any conditional nested inside the
                        // return type (e.g. `Collection<($k is
                        // array|string ? array-key : TGroupKey), …>`)
                        // against this call's arguments.  The hint
                        // becomes the receiver type of the next link
                        // in the chain, so a conditional left raw here
                        // ends up bound to a class-level template
                        // parameter and is later compared against an
                        // argument as an uninhabited type expression.
                        let substituted = if substituted.contains_conditional() {
                            let arg_ty_resolver = |t: &str| Self::resolve_arg_text_to_type(t, ctx);
                            let tpl = TemplateContext {
                                defaults: Some(&template_subs),
                                params: &m.template_params,
                                bindings: &m.template_bindings,
                                arg_type_resolver: Some(&arg_ty_resolver),
                            };
                            crate::type_engine::conditional_resolution::evaluate_nested_conditionals_text(
                                &substituted,
                                &m.parameters,
                                text_args,
                                Some(&var_resolver),
                                crate::type_engine::conditional_resolution::ConditionalClassContext {
                                    calling: ctx.current_class.map(|c| c.name.as_str()),
                                    declaring: Some(merged.fqn().as_str()),
                                },
                                ctx.class_loader,
                                &tpl,
                            )
                        } else {
                            substituted
                        };
                        // Resolve self/static/parent keywords to
                        // concrete class names so that downstream
                        // consumers see real FQNs, not keywords.
                        // Prefer the receiver's full generic type
                        // (e.g. Builder<User>) so fluent chains like
                        // where()->lockForUpdate()->firstOrFail()
                        // keep TModel.
                        **hint_out = Some(resolve_hint_keywords(
                            substituted,
                            &owner,
                            ctx.class_loader,
                            |s| match &rt.type_string.kind() {
                                TypeKind::Generic(_) => s.replace_self_with_type(&rt.type_string),
                                _ => s.replace_self(&owner.fqn()),
                            },
                        ));
                    }
                    hint_captured = true;
                }
            }
            let mr_ctx = MethodReturnCtx::for_call(ctx, &template_subs, &var_resolver, false);
            if let Some((date_class, date_return_type)) =
                Self::configured_laravel_date_return(&owner, method_name, ctx.class_loader)
            {
                ClassInfo::push_unique_arc(&mut results, date_class);
                if let Some(ref mut hint_out) = return_type_hint_out {
                    **hint_out = Some(date_return_type);
                }
            } else {
                // Dedup by class name: a union receiver whose members
                // all declare the same return type (e.g. a fluent
                // chain through `Expectation|HigherOrderExpectation`)
                // would otherwise double the result set at every
                // link, growing 2^n over the chain.
                ClassInfo::extend_unique_arc(
                    &mut results,
                    Self::resolve_method_return_types_with_args(
                        &owner,
                        method_name,
                        text_args,
                        &mr_ctx,
                    ),
                );
            }
        }
        results
    }

    /// `Class::method(…)`: the static method call arm.
    pub(super) fn return_types_of_static_method_call(
        class: &str,
        method_name: &str,
        text_args: &str,
        ctx: &ResolutionCtx<'_>,
        mut return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        let owner_class = if class.starts_with('$') {
            // Variable holding a class-string (e.g. `$cls::make()`).
            // May resolve to multiple classes for union class-strings.
            let all_owners: Vec<Arc<ClassInfo>> = ResolvedType::into_arced_classes(
                crate::type_engine::resolver::resolve_target_classes(
                    class,
                    AccessKind::DoubleColon,
                    ctx,
                ),
            );
            // When there are multiple possible classes, resolve the
            // method return type through each and union the results.
            if all_owners.len() > 1 {
                let mut union_results: Vec<Arc<ClassInfo>> = Vec::new();
                for owner in &all_owners {
                    let split_args = split_text_args(text_args);
                    let arg_refs = split_args.to_vec();
                    let template_subs =
                        Self::build_method_template_subs(owner, method_name, &arg_refs, ctx);
                    let var_resolver = build_var_resolver(ctx);
                    let mr_ctx =
                        MethodReturnCtx::for_call(ctx, &template_subs, &var_resolver, true);
                    ClassInfo::extend_unique_arc(
                        &mut union_results,
                        Self::resolve_method_return_types_with_args(
                            owner,
                            method_name,
                            text_args,
                            &mr_ctx,
                        ),
                    );
                }
                if !union_results.is_empty() {
                    return union_results;
                }
            }
            all_owners.into_iter().next()
        } else {
            crate::type_engine::resolver::resolve_static_owner_class(class, ctx)
        };

        if let Some(ref owner) = owner_class {
            // A static call through a Laravel facade is typed by the
            // container class the facade forwards to, so that
            // `App::make(Foo::class)->…` sees the same
            // argument-dependent return the assignment path does.
            let concrete_owner = facade_concrete_owner(
                owner,
                method_name,
                ctx.class_loader,
                ctx.resolved_class_cache,
                ctx.backend,
            );
            let owner = concrete_owner.as_ref().unwrap_or(owner);

            // Fully resolve the owner so post-resolution patches
            // (e.g. Laravel facade return-type corrections) and
            // inherited / interface-merged members are visible.
            // The static path otherwise reads the raw parsed class,
            // whose own real methods shadow the patched versions
            // that only exist on the merged class.  The call is
            // cached, so it doesn't duplicate work.
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                owner,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );

            let split_args = split_text_args(text_args);
            let arg_refs = split_args.to_vec();
            let template_subs =
                Self::build_method_template_subs(&merged, method_name, &arg_refs, ctx);

            if let Some(ref mut hint_out) = return_type_hint_out
                && let Some(m) = merged.get_method_ci(method_name)
                && let Some(ref ret) = m.return_type
            {
                // Bind the method's own @template params from the
                // call-site arguments before the hint travels on as
                // the receiver of the next link in the chain.  A
                // factory declared `@return static<array-key, T>`
                // otherwise hands the next call a raw `T`.
                let substituted = if template_subs.is_empty() {
                    ret.clone()
                } else {
                    ret.substitute(&template_subs)
                };
                // Resolve self/static/parent keywords to concrete
                // class names (mirrors instance path). Only the
                // `static`/`self` name is replaced, so
                // `static<array-key, string>` keeps its bound
                // arguments instead of collapsing to a bare class
                // name.
                **hint_out = Some(resolve_hint_keywords(
                    substituted,
                    &merged,
                    ctx.class_loader,
                    |s| s.replace_self(&merged.fqn()),
                ));
            }

            let var_resolver = build_var_resolver(ctx);
            let mr_ctx = MethodReturnCtx::for_call(ctx, &template_subs, &var_resolver, true);
            if let Some((date_class, date_return_type)) =
                Self::configured_laravel_date_return(&merged, method_name, ctx.class_loader)
            {
                if let Some(ref mut hint_out) = return_type_hint_out {
                    **hint_out = Some(date_return_type);
                }
                return vec![date_class];
            }
            return Self::resolve_method_return_types_with_args(
                &merged,
                method_name,
                text_args,
                &mr_ctx,
            );
        }
        vec![]
    }

    /// `app(…)` / `myHelper(…)`: the standalone function call arm.
    pub(super) fn return_types_of_function_call(
        func_name: &str,
        text_args: &str,
        ctx: &ResolutionCtx<'_>,
        mut return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        // ── Laravel container string binding ────────────────
        // `app('blade.compiler')` / `resolve('cache')` bind a plain
        // string to a concrete class.  The class-string form
        // (`app(User::class)`) is handled by the conditional return
        // type below; only a literal string binding is intercepted
        // here, resolved via the framework's own alias table.
        let normalized_func = func_name.trim_start_matches('\\');
        if matches!(normalized_func, "app" | "resolve")
            && let Some(binding) = Self::extract_first_arg_text(text_args)
            && let Some(name) = crate::util::unescape_php_string_literal(binding.trim())
            && let Some(cls) = (ctx.class_loader)(&name)
        {
            return vec![cls];
        }

        // ── now() / today() → configured Laravel date class ──
        // The global `now()`/`today()` helpers are declared to
        // return `CarbonInterface`, but they actually instantiate the
        // concrete class selected by Laravel's date factory. Resolving
        // to the interface loses the
        // concrete type and produces spurious mismatches when a
        // chained call is assigned to a `DateTime`/`DateTimeImmutable`
        // declaration.  Map both to the concrete class.  Only applies
        // when the class is loadable (i.e. inside a Laravel project).
        //
        // Not strictly sound (the declared type is the interface),
        // but it is what the Laravel PHPStan extensions infer too, and
        // the ecosystem is written against it.  See the matching note in
        // `rhs_resolution.rs`.
        if matches!(
            normalized_func,
            "now" | "today" | "Illuminate\\Support\\now" | "Illuminate\\Support\\today"
        ) && let Some(cls) =
            (ctx.class_loader)(crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)
        {
            return vec![cls];
        }

        // ── view('name') → concrete Illuminate\View\View ─────
        if crate::virtual_members::laravel::view_helper_returns_view(
            normalized_func,
            Self::extract_first_arg_text(text_args)
                .as_deref()
                .unwrap_or(""),
        ) && let Some(cls) = (ctx.class_loader)(crate::virtual_members::laravel::VIEW_FQN)
        {
            return vec![cls];
        }

        // ── Array-producing / element-extracting functions ───
        // The stubs declare these as returning a bare `array` or
        // `mixed`, so the element-type rules in
        // `variable::array_func_rules` supply the real type.
        // The same rules run on the AST path when the call is an
        // assignment right-hand side; here they cover every
        // inline use (`array_map(…)[0]`, `f(array_filter(…))`).
        if !text_args.is_empty() {
            let owner_name = ctx.current_class.map(|c| c.name.as_str()).unwrap_or("");
            let fn_args = TextArrayFuncArgs::new(text_args, ctx);

            // String builtins over literal arguments: the stub
            // declares the widest string the function can return,
            // but a call whose arguments are all literals has one
            // answer.  It names no class, so it travels purely as
            // the hint.
            if crate::type_engine::variable::string_func_rules::is_foldable_string_func(func_name)
                && let Some(folded) =
                    crate::type_engine::variable::string_func_rules::string_func_literal_type(
                        func_name, &fn_args,
                    )
            {
                if let Some(ref mut hint_out) = return_type_hint_out {
                    **hint_out = Some(folded);
                }
                return Vec::new();
            }

            // Element-extracting functions (`array_pop`, `current`,
            // …): the call's type *is* the element type.
            if let Some(element_type) = array_func_element_type(func_name, &fn_args) {
                let classes: Vec<Arc<ClassInfo>> =
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        &element_type,
                        owner_name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                if let Some(ref mut hint_out) = return_type_hint_out {
                    **hint_out = Some(element_type);
                }
                return classes;
            }

            // Array-producing functions (`array_filter`,
            // `array_map`, `iterator_to_array`, …): the container
            // type is what a caller indexing into the call
            // (`array_map(…)[0]`) needs, so it travels as the
            // hint.  The classes stay the element's, since an
            // array has none of its own.
            //
            // Both rules answer for the whole call, so they return
            // even with no classes to report: an element that names
            // no class (a scalar, an `array{…}` shape) still leaves
            // the hint carrying the real type.  Falling through
            // would overwrite it with the stub's bare `array` /
            // `mixed`, which is what these rules exist to replace.
            if let Some(raw_type) = array_func_raw_type(func_name, &fn_args) {
                let classes: Vec<Arc<ClassInfo>> = raw_type
                    .extract_value_type(true)
                    .map(|element_type| {
                        crate::type_engine::type_resolution::type_hint_to_classes_typed(
                            element_type,
                            owner_name,
                            ctx.all_classes,
                            ctx.class_loader,
                        )
                    })
                    .unwrap_or_default();
                if let Some(ref mut hint_out) = return_type_hint_out {
                    **hint_out = Some(raw_type);
                }
                return classes;
            }
        }

        if let Some(fl) = ctx.function_loader
            && let Some(func_info) = fl(func_name, 0)
        {
            if func_info.conditional_return.is_some()
                || crate::type_engine::types::flag_returns::has_flag_dependent_return(func_name)
            {
                let var_resolver = build_var_resolver(ctx);
                // `is <Type>` conditions on an argument that isn't a
                // literal (`preg_replace($p, $r, $subject)`) are
                // decided by the argument's resolved type, so the
                // branch a call takes matches what it was handed.
                let arg_ty_resolver = |t: &str| Self::resolve_arg_text_to_type(t, ctx);
                let resolved_type = func_info
                    .conditional_return
                    .as_ref()
                    .and_then(|cond| {
                        if text_args.is_empty() {
                            return resolve_conditional_without_args(cond, &func_info.parameters);
                        }
                        let tpl = TemplateContext {
                            defaults: None,
                            params: &func_info.template_params,
                            bindings: &func_info.template_bindings,
                            arg_type_resolver: Some(&arg_ty_resolver),
                        };
                        resolve_conditional_with_text_args(
                            cond,
                            &func_info.parameters,
                            text_args,
                            Some(&var_resolver),
                            ctx.current_class.map(|c| c.name.as_str()),
                            ctx.class_loader,
                            &tpl,
                        )
                    })
                    // A branch the flags argument rules out
                    // (`json_encode(…, JSON_THROW_ON_ERROR)` never
                    // returning `false`) is decided the same way: at
                    // the call site, from the declared return type.
                    .or_else(|| {
                        crate::type_engine::types::flag_returns::flag_narrowed_return_type(
                            func_name,
                            &func_info.parameters,
                            text_args,
                            func_info.return_type.as_ref()?,
                            Some(&arg_ty_resolver),
                        )
                    });
                if let Some(parsed_ty) = resolved_type {
                    // The winning branch can name a function-level
                    // `@template` (`tap()` returns `TValue`), which
                    // only the call-site arguments fill in.
                    let parsed_ty =
                        crate::type_engine::variable::rhs_resolution::substitute_function_templates(
                            &func_info,
                            parsed_ty,
                            &split_text_args(text_args)
                                .into_iter()
                                .map(str::to_string)
                                .collect::<Vec<String>>(),
                            None,
                            ctx,
                        );
                    let classes: Vec<Arc<ClassInfo>> =
                        crate::type_engine::type_resolution::type_hint_to_classes_typed(
                            &parsed_ty,
                            "",
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    // The collapsed conditional is the call's real
                    // type; report it even when it names no class
                    // (`string`, `list<string>`, an array shape), or
                    // callers that need the type string rather than a
                    // `ClassInfo` fall back to the declared
                    // `array`/`mixed` below.
                    //
                    // A branch that collapsed to bare `mixed` decided
                    // nothing, though.  The function-level `@template`
                    // substitution below still binds the parameter's
                    // default, which is what makes an argument-less
                    // `app()` its `Application::class` default rather
                    // than `mixed`, so leave the answer to it.
                    if !classes.is_empty() || !parsed_ty.is_mixed() {
                        if let Some(ref mut hint_out) = return_type_hint_out {
                            **hint_out = Some(parsed_ty);
                        }
                        return classes;
                    }
                }
            }
            // ── Function-level @template substitution ────────
            // When the function has template params and bindings,
            // infer concrete types from the arguments and apply
            // substitution to the return type before resolving.
            // Delegates to `build_function_template_subs` which
            // handles Direct, ArrayElement, and GenericWrapper
            // binding modes (e.g. `@param array<TKey, TValue>`).
            if !func_info.template_params.is_empty() && func_info.return_type.is_some() {
                let split_args: Vec<String> = if text_args.is_empty() {
                    vec![]
                } else {
                    split_text_args(text_args)
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                };
                let subs =
                    crate::type_engine::variable::rhs_resolution::build_function_template_subs(
                        &func_info,
                        &split_args,
                        None,
                        ctx,
                    );

                if !subs.is_empty()
                    && let Some(ref ret) = func_info.return_type
                {
                    let substituted = ret.substitute(&subs);
                    let classes: Vec<Arc<ClassInfo>> =
                        crate::type_engine::type_resolution::type_hint_to_classes_typed(
                            &substituted,
                            "",
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    // Report the substituted type, not the raw
                    // `@return T[]` the fallback below would
                    // write: an unbound template param is
                    // useless to a caller reading the hint.
                    let bound = substituted != *ret;
                    if bound && let Some(ref mut hint_out) = return_type_hint_out {
                        **hint_out = Some(substituted);
                    }
                    if !classes.is_empty() {
                        return classes;
                    }
                    // A bound return that names no class of its own
                    // (`array_values(array<int, Product>)` is
                    // `list<Product>`; `array_keys(…)` is
                    // `list<int>`) still answers for the whole call.
                    // Falling through would overwrite the hint just
                    // set with the declared `list<TValue>`, handing
                    // the caller an unbound template parameter in
                    // place of the type it resolved.
                    if bound {
                        return classes;
                    }
                }
            }

            if let Some(ref ret) = func_info.return_type {
                if let Some(ref mut hint_out) = return_type_hint_out {
                    **hint_out = Some(ret.clone());
                }
                return crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    ret,
                    "",
                    ctx.all_classes,
                    ctx.class_loader,
                );
            }
        }

        vec![]
    }

    /// `$fn(…)`: the variable invocation arm.
    pub(super) fn return_types_of_variable_invocation(
        var_name: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Vec<Arc<ClassInfo>> {
        let content = ctx.content;
        let cursor_offset = ctx.cursor_offset;

        // 1. Try docblock annotation: `@var Closure(): User $fn`
        if let Some(raw_type) = crate::docblock::find_iterable_raw_type_in_source(
            content,
            cursor_offset as usize,
            var_name,
        )
        .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
            && let Some(ret_type) = raw_type.callable_return_type()
        {
            let classes: Vec<Arc<ClassInfo>> =
                crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    ret_type,
                    "",
                    ctx.all_classes,
                    ctx.class_loader,
                );
            if !classes.is_empty() {
                return classes;
            }
        }

        // 2. Resolve the variable's own type.  Closures, arrow
        //    functions, and first-class callables are all
        //    inferred as a `TypeKind::Callable` (see
        //    `infer_closure_literal_type`), so `$fn`'s embedded
        //    return type covers `$fn = function(): T {}`,
        //    `$fn = fn(): T => …`, and `$fn = strlen(...)` /
        //    `$fn = $obj->method(...)` alike.
        let resolved_var_types =
            crate::type_engine::resolver::resolve_target_classes(var_name, AccessKind::Arrow, ctx);
        for rt in &resolved_var_types {
            if let Some(ret_type) = rt.type_string.callable_return_type() {
                let classes: Vec<Arc<ClassInfo>> =
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        ret_type,
                        "",
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                if !classes.is_empty() {
                    return classes;
                }
            }
        }

        // 3. Check for __invoke().  When $f holds an object with
        //    an __invoke() method, $f() should return
        //    __invoke()'s return type.
        let var_classes = ResolvedType::into_arced_classes(resolved_var_types);
        for owner in &var_classes {
            if let Some(invoke) = owner.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let classes: Vec<Arc<ClassInfo>> =
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        ret,
                        "",
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                if !classes.is_empty() {
                    return classes;
                }
            }
        }

        vec![]
    }

    /// `new ClassName(…)`: the constructor call arm.  The return type is
    /// always the class itself; when the class has `@template` params and
    /// the constructor binds them, concrete types are inferred from
    /// `text_args` so that chained calls like `(new C("foo"))->get()`
    /// propagate generics correctly.
    pub(super) fn return_types_of_constructor_call(
        class_name: &str,
        text_args: &str,
        ctx: &ResolutionCtx<'_>,
        mut return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        // `new X` is a source-level reference: an unqualified name
        // resolves against the current namespace before the global
        // scope, so a same-namespace class wins over a global stub
        // of the same short name.
        let ns = ctx.current_class.and_then(|c| c.file_namespace.as_deref());
        let fqn = crate::util::resolve_source_class_name(class_name, ns, ctx.class_loader);
        let cls_arc = find_class_by_name(ctx.all_classes, class_name)
            .map(Arc::clone)
            .or_else(|| (ctx.class_loader)(&fqn));
        let cls_arc = match cls_arc {
            Some(c) => c,
            None => return vec![],
        };

        // `new ReflectionProperty(C::class, 'name')` is the value
        // `ReflectionClass::getProperty('name')` builds, written
        // the other way, so it carries the same class and name.
        if is_reflected_property_class(cls_arc.fqn().as_str())
            && let Some(ty) =
                resolve_reflected_property_at_new(&cls_arc, &split_text_args(text_args), ctx)
        {
            if let Some(ref mut hint_out) = return_type_hint_out {
                **hint_out = Some(ty);
            }
            return vec![cls_arc];
        }

        // Fast path: no template params, no inference needed.
        if cls_arc.template_params.is_empty() || text_args.is_empty() {
            return vec![cls_arc];
        }

        // Find the constructor (on this class or an ancestor).
        let ancestor_arc;
        let ctor_inherited;
        let ctor_ref = if let Some(c) = cls_arc.get_method("__construct") {
            ctor_inherited = false;
            Some(c)
        } else {
            let mut found: Option<Arc<ClassInfo>> = None;
            let mut cur = cls_arc.parent_class.as_ref().map(|p| p.to_string());
            for _ in 0..15 {
                let parent_name = match cur {
                    Some(ref n) => n.clone(),
                    None => break,
                };
                if let Some(parent) = (ctx.class_loader)(&parent_name) {
                    if parent.get_method("__construct").is_some() {
                        found = Some(parent);
                        break;
                    }
                    cur = parent.parent_class.as_ref().map(|p| p.to_string());
                } else {
                    break;
                }
            }
            match found {
                Some(arc) => {
                    ancestor_arc = arc;
                    ctor_inherited = true;
                    ancestor_arc.get_method("__construct")
                }
                None => {
                    ctor_inherited = false;
                    None
                }
            }
        };

        if let Some(ctor) = ctor_ref
            && !ctor.template_bindings.is_empty()
        {
            let arg_texts = crate::type_engine::conditional_resolution::split_text_args(text_args);
            if !arg_texts.is_empty() {
                // The finishing half of `build_method_template_subs`
                // (filling a param nothing bound) is skipped here: it
                // would fall back to the *constructor's* own
                // `@template … of …` bound, which a constructor
                // essentially never repeats — the bound lives on the
                // class. `cls_arc.template_param_bounds` is used for
                // that below instead.
                let subs = Backend::bind_method_template_args(ctor, &arg_texts, ctx);

                // Remap inherited constructor subs to the child's
                // template param names via the @extends chain.
                let effective_subs = if ctor_inherited && !subs.is_empty() {
                    crate::type_engine::variable::rhs_resolution::remap_inherited_ctor_subs(
                        &cls_arc,
                        &subs,
                        ctx.class_loader,
                    )
                } else {
                    subs
                };

                if !effective_subs.is_empty() {
                    let type_args: Vec<PhpType> = cls_arc
                        .template_params
                        .iter()
                        .map(|p| {
                            let p_str: &str = p.as_ref();
                            effective_subs.get(p_str).cloned().unwrap_or_else(|| {
                                cls_arc
                                    .template_param_bounds
                                    .get(p)
                                    .cloned()
                                    .unwrap_or_else(PhpType::mixed)
                            })
                        })
                        .collect();
                    let substituted = crate::virtual_members::resolve_class_fully_with_type_args(
                        &cls_arc,
                        ctx.class_loader,
                        ctx.resolved_class_cache,
                        &type_args,
                    );
                    if let Some(ref mut hint_out) = return_type_hint_out {
                        **hint_out = Some(PhpType::generic_atom(substituted.fqn(), type_args));
                    }
                    return vec![substituted];
                }
            }
        }

        // Fallback: resolve omitted template params to their defaults
        // and otherwise erase them to their bounds.
        let type_args = crate::inheritance::default_type_args(&cls_arc);
        let substituted = crate::virtual_members::resolve_class_fully_with_type_args(
            &cls_arc,
            ctx.class_loader,
            ctx.resolved_class_cache,
            &type_args,
        );
        if let Some(ref mut hint_out) = return_type_hint_out {
            **hint_out = Some(PhpType::generic_atom(substituted.fqn(), type_args));
        }
        vec![substituted]
    }

    /// Any other callee form: a nested `CallExpr` used as a callee, a
    /// `PropertyChain` for `($this->prop)()`, or a `ClassName` that
    /// `SubjectExpr::parse` could not distinguish from a function name.
    pub(super) fn return_types_of_other_callee(
        callee: &SubjectExpr,
        ctx: &ResolutionCtx<'_>,
        mut return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        let callee_resolved = crate::type_engine::resolver::resolve_target_classes_expr(
            callee,
            AccessKind::Arrow,
            ctx,
        );

        // A callable-typed callee carries its return type in the
        // type string rather than on a class, which is how a
        // property annotated `@var callable(): Scope` arrives
        // here.  Read it the same way the `$fn(…)` path does.
        // Subject resolution keeps only class-typed results, so a
        // property whose type is a bare `callable(…): T` comes back
        // empty and its declared hint has to be read directly.
        let mut callable_types: Vec<PhpType> = callee_resolved
            .iter()
            .map(|rt| rt.type_string.clone())
            .collect();
        if callable_types.is_empty()
            && let SubjectExpr::PropertyChain { base, property } = callee
        {
            let owners = ResolvedType::into_arced_classes(
                crate::type_engine::resolver::resolve_target_classes_expr(
                    base,
                    AccessKind::Arrow,
                    ctx,
                ),
            );
            for owner in &owners {
                if let Some(hint) = crate::inheritance::resolve_property_type_hint(
                    owner,
                    property,
                    ctx.class_loader,
                ) {
                    callable_types.push(hint);
                }
            }
        }
        for ty in &callable_types {
            if let Some(ret_type) = ty.callable_return_type() {
                let classes: Vec<Arc<ClassInfo>> =
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        ret_type,
                        "",
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                if !classes.is_empty() {
                    if let Some(ref mut hint_out) = return_type_hint_out {
                        **hint_out = Some(ret_type.clone());
                    }
                    return classes;
                }
            }
        }

        let callee_classes = ResolvedType::into_arced_classes(callee_resolved);

        // When the callee resolves to an object with __invoke(),
        // the call returns __invoke()'s return type, not the
        // object itself.  This handles `($this->formatter)()`.
        for owner in &callee_classes {
            if let Some(invoke) = owner.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let classes: Vec<Arc<ClassInfo>> =
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        ret,
                        "",
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                if !classes.is_empty() {
                    return classes;
                }
            }
        }

        callee_classes
    }
}
