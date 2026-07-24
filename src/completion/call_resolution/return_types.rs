/// Call return-type resolution: the primary entry point that resolves a
/// structured call expression + argument text to zero or more `ClassInfo`
/// values, plus the auth/date facade helpers and literal/expression-to-type
/// conversions it depends on.
use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::completion::variable::{ARRAY_ELEMENT_FUNCS, ARRAY_PRESERVING_FUNCS};
use crate::php_type::PhpType;
use crate::subject_expr::SubjectExpr;
use crate::types::ClassLikeKind;
use crate::types::*;
use crate::util::find_class_by_name;
use crate::util::{is_self_or_static, resolve_class_keyword};

use crate::completion::conditional_resolution::{
    TemplateContext, VarClassStringResolver, resolve_conditional_with_text_args,
    resolve_conditional_with_text_args_and_defaults, resolve_conditional_without_args,
    resolve_conditional_without_args_and_defaults, split_text_args,
};
use crate::completion::resolver::ResolutionCtx;

use super::target_cache::{AUTH_USER_RESOLVER, try_infer_body_return_type};

/// Bundled parameters for [`Backend::resolve_method_return_types_with_args`].
///
/// Groups the resolution-context fields that are threaded through method
/// return-type resolution so the function stays within clippy's argument
/// limit.
pub(crate) struct MethodReturnCtx<'a> {
    /// All classes known in the current file.
    pub all_classes: &'a [Arc<ClassInfo>],
    /// Cross-file class resolution callback.
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Template substitution map (method-level `@template` bindings).
    pub template_subs: &'a HashMap<String, PhpType>,
    /// Resolves a variable name to class-string values (for conditional
    /// return type evaluation).
    pub var_resolver: VarClassStringResolver<'a>,
    /// Shared resolved-class cache (when available).
    pub cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// The class at the call site (where `self::class` / `static::class`
    /// appears), as opposed to the class that owns the method being called.
    /// Used to resolve `self`/`static`/`parent` in conditional return types.
    pub calling_class_name: Option<&'a str>,
    /// Whether the call is a static method call (`Class::method()`).
    ///
    /// When `true`, the magic-method fallback checks `__callStatic`
    /// instead of `__call`.
    pub is_static: bool,
}

/// Build a [`VarClassStringResolver`] closure from a [`ResolutionCtx`].
///
/// The returned closure resolves a variable name (e.g. `"$requestType"`)
/// to the class names it holds as class-string values by delegating to
/// [`resolve_class_string_targets`](crate::completion::variable::class_string_resolution::resolve_class_string_targets).
pub(super) fn build_var_resolver<'a>(
    ctx: &'a ResolutionCtx<'a>,
) -> impl Fn(&str) -> Vec<String> + 'a {
    move |var_name: &str| -> Vec<String> {
        if let Some(cc) = ctx.current_class {
            crate::completion::variable::class_string_resolution::resolve_class_string_targets(
                var_name,
                cc,
                ctx.all_classes,
                ctx.content,
                ctx.cursor_offset,
                ctx.class_loader,
            )
            .iter()
            .map(|c| c.name.to_string())
            .collect()
        } else {
            vec![]
        }
    }
}

/// Resolve a `user()` call on an auth entry point to the model type
/// configured for the guard named at the call site.
///
/// Returns `None` (so the caller falls back to ordinary method
/// resolution, which keeps the default-guard class-level patch) when:
///
/// * the receiver is not a `Guard`/`Request` subtype (so this is some
///   unrelated `user()` method),
/// * no [`AUTH_USER_RESOLVER`] is active on this thread, or
/// * the guard's provider maps to no concrete model.
///
/// `base` is the receiver expression (used to recover the guard name
/// from `auth('admin')` / `Auth::guard('admin')` / `->guard('admin')`),
/// and `user_args` is the argument text of the `user()` call itself
/// (used to recover the guard name from `$request->user('admin')`).
fn resolve_auth_user_at_call(
    base: &SubjectExpr,
    user_args: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<Arc<ClassInfo>>> {
    // Cheap gate first: without an active resolver there is nothing to
    // refine, so skip the (comparatively expensive) subtype walk below.
    let is_resolver_active = AUTH_USER_RESOLVER.with(|cell| cell.borrow().is_some());
    if !is_resolver_active {
        return None;
    }

    // Only intercept `user()` on an actual auth entry point.  Every
    // other class with a `user()` method must resolve normally.
    let is_auth_receiver = owners.iter().any(|rt| {
        rt.class_info.as_ref().is_some_and(|ci| {
            crate::util::is_subtype_of(
                ci,
                crate::virtual_members::laravel::GUARD_FQN,
                ctx.class_loader,
            ) || crate::util::is_subtype_of(
                ci,
                crate::virtual_members::laravel::REQUEST_FQN,
                ctx.class_loader,
            )
        })
    });
    if !is_auth_receiver {
        return None;
    }

    let guard = auth_guard_name(base, user_args);
    let model_type =
        AUTH_USER_RESOLVER.with(|cell| cell.borrow().as_ref().and_then(|f| f(guard.as_deref())))?;

    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        &model_type,
        "",
        ctx.all_classes,
        ctx.class_loader,
    );
    if classes.is_empty() {
        None
    } else {
        Some(classes)
    }
}

/// Recover the guard name from a `user()` call site.
///
/// The guard name may be an explicit argument to `user()` itself
/// (`$request->user('admin')`) or come from the auth entry point that
/// produced the receiver (`auth('admin')`, `Auth::guard('admin')`,
/// `auth()->guard('admin')`).  Returns `None` for the default guard or
/// when the guard argument is not a plain string literal (a runtime
/// value we cannot pin down statically).
fn auth_guard_name(base: &SubjectExpr, user_args: &str) -> Option<String> {
    // Explicit guard argument on `user()` itself.
    if let Some(name) = first_string_literal_arg(user_args) {
        return Some(name);
    }
    // Guard name carried by the receiver expression.
    if let SubjectExpr::CallExpr { callee, args_text } = base {
        match callee.as_ref() {
            // `auth('admin')` global helper.
            SubjectExpr::FunctionCall(name)
                if name.trim_start_matches('\\').eq_ignore_ascii_case("auth") =>
            {
                return first_string_literal_arg(args_text);
            }
            // `Auth::guard('admin')` facade, or `auth()->guard('admin')` /
            // `$factory->guard('admin')`.  The receiver-subtype gate above
            // has already confirmed the resulting value is a `Guard`.
            SubjectExpr::StaticMethodCall { method, .. }
            | SubjectExpr::MethodCall { method, .. }
                if method.eq_ignore_ascii_case("guard") =>
            {
                return first_string_literal_arg(args_text);
            }
            _ => {}
        }
    }
    None
}

/// Extract the first argument of a call as a plain string literal.
///
/// Returns `None` when there are no arguments or the first argument is
/// not a single-quoted or double-quoted string literal.
fn first_string_literal_arg(args_text: &str) -> Option<String> {
    let first = split_text_args(args_text).into_iter().next()?;
    crate::util::unquote_php_string(first.trim()).map(str::to_string)
}

fn replace_support_carbon_return(ty: &PhpType, configured_class: &str) -> Option<PhpType> {
    match ty {
        PhpType::Named(name) => (name.trim_start_matches('\\')
            == crate::virtual_members::laravel::SUPPORT_CARBON_FQN)
            .then(|| PhpType::Named(configured_class.to_string())),
        PhpType::Nullable(inner) => replace_support_carbon_return(inner, configured_class)
            .map(|inner| PhpType::Nullable(Box::new(inner))),
        PhpType::Union(members) => {
            let mut replaced = false;
            let members = members
                .iter()
                .map(
                    |member| match replace_support_carbon_return(member, configured_class) {
                        Some(member) => {
                            replaced = true;
                            member
                        }
                        None => member.clone(),
                    },
                )
                .collect();
            replaced.then_some(PhpType::Union(members))
        }
        _ => None,
    }
}

impl Backend {
    pub(crate) fn configured_laravel_date_return(
        owner: &ClassInfo,
        method_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<(Arc<ClassInfo>, PhpType)> {
        if !matches!(
            owner.fqn().as_str(),
            "Illuminate\\Support\\Facades\\Date" | "Illuminate\\Support\\DateFactory"
        ) {
            return None;
        }
        let return_type = owner
            .get_method_ci(method_name)
            .and_then(|method| method.return_type.as_ref())?;
        let date_class = class_loader(crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)?;
        let return_type = replace_support_carbon_return(return_type, date_class.fqn().as_str())?;

        Some((date_class, return_type))
    }
}

impl Backend {
    /// Resolve the return type of a call expression given a structured
    /// [`SubjectExpr`] callee and argument text, returning zero or more
    /// `ClassInfo` values.
    ///
    /// This is the primary entry point for call return type resolution.
    /// The callee should be one of the "callee" variants produced by
    /// `parse_callee`: [`SubjectExpr::MethodCall`],
    /// [`SubjectExpr::StaticMethodCall`], [`SubjectExpr::FunctionCall`],
    /// [`SubjectExpr::Variable`], or [`SubjectExpr::NewExpr`].
    /// Any other variant falls through to `resolve_target_classes_expr`.
    ///
    /// Resolves the return type of a structured [`SubjectExpr`] callee +
    /// argument text.  Optionally captures the raw return type hint
    /// (with template substitutions applied) into `return_type_hint_out`
    /// when provided.  This preserves generic
    /// type parameters (e.g. `HasMany<Translation, Tag>`) that would
    /// otherwise be lost when converting to `Vec<Arc<ClassInfo>>`.
    pub(crate) fn resolve_call_return_types_expr_with_hint(
        callee: &SubjectExpr,
        text_args: &str,
        ctx: &ResolutionCtx<'_>,
        mut return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        match callee {
            // ── Instance method call: base->method(…) ───────────────
            SubjectExpr::MethodCall { base, method } => {
                let method_name = method.as_str();

                // Resolve the base expression preserving generic type
                // arguments (e.g. `Collection<Product>`) so class-level
                // template parameters can be substituted in the method's
                // return type.
                let lhs_resolved: Vec<ResolvedType> =
                    crate::completion::resolver::resolve_target_classes_expr(
                        base,
                        AccessKind::Arrow,
                        ctx,
                    );

                // Guard-aware auth user model: a `user()` call on a
                // `Guard`/`Request` subtype resolves to the model
                // configured for the guard named at the call site
                // (`auth('admin')`, `Auth::guard('admin')`,
                // `$request->user('admin')`), falling back to the
                // default-guard model otherwise.
                if method_name == "user"
                    && let Some(classes) =
                        resolve_auth_user_at_call(base, text_args, &lhs_resolved, ctx)
                {
                    return classes;
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
                    let class_level_subs: HashMap<String, PhpType> = match &rt.type_string {
                        PhpType::Generic(_, args)
                            if !args.is_empty()
                                && !owner.template_params.is_empty()
                                && !args.iter().any(|a| a.is_self_like()) =>
                        {
                            owner
                                .template_params
                                .iter()
                                .zip(args.iter())
                                .map(|(name, ty)| (name.to_string(), ty.clone()))
                                .collect()
                        }
                        _ => HashMap::new(),
                    };

                    let split_args = split_text_args(text_args);
                    let arg_refs = split_args.to_vec();
                    let method_subs =
                        Self::build_method_template_subs(&owner, method_name, &arg_refs, ctx);

                    // Merge class-level generic substitutions with
                    // method-level template substitutions.  Class-level
                    // subs map e.g. `TItem → Product`; method-level subs
                    // map method @template params from call-site args.
                    // Method-level subs take precedence (inserted last).
                    let mut template_subs = class_level_subs;
                    template_subs.extend(method_subs);

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
                            if let Some(ref ret) = m.return_type {
                                let substituted = if !template_subs.is_empty() {
                                    ret.substitute(&template_subs)
                                } else {
                                    ret.clone()
                                };
                                // Resolve self/static/parent keywords to
                                // concrete class names so that downstream
                                // consumers see real FQNs, not keywords.
                                // Prefer the receiver's full generic type
                                // (e.g. Builder<User>) so fluent chains like
                                // where()->lockForUpdate()->firstOrFail()
                                // keep TModel.
                                let resolved_hint = if substituted.is_parent_ref() {
                                    owner
                                        .parent_class
                                        .as_ref()
                                        .map(|p| PhpType::Named(p.to_string()))
                                        .unwrap_or(substituted)
                                } else if substituted.contains_self_ref() {
                                    match &rt.type_string {
                                        PhpType::Generic(_, _) => {
                                            substituted.replace_self_with_type(&rt.type_string)
                                        }
                                        _ => substituted.replace_self(&owner.fqn()),
                                    }
                                } else {
                                    substituted
                                };
                                **hint_out = Some(resolved_hint);
                            }
                            hint_captured = true;
                        }
                    }
                    let var_resolver = build_var_resolver(ctx);
                    let mr_ctx = MethodReturnCtx {
                        all_classes: ctx.all_classes,
                        class_loader: ctx.class_loader,
                        template_subs: &template_subs,
                        var_resolver: Some(&var_resolver),
                        cache: ctx.resolved_class_cache,
                        calling_class_name: ctx.current_class.map(|c| c.name.as_str()),
                        is_static: false,
                    };
                    if let Some((date_class, date_return_type)) =
                        Self::configured_laravel_date_return(&owner, method_name, ctx.class_loader)
                    {
                        results.push(date_class);
                        if let Some(ref mut hint_out) = return_type_hint_out {
                            **hint_out = Some(date_return_type);
                        }
                    } else {
                        results.extend(Self::resolve_method_return_types_with_args(
                            &owner,
                            method_name,
                            text_args,
                            &mr_ctx,
                        ));
                    }
                }
                results
            }

            // ── Static method call: Class::method(…) ────────────────
            SubjectExpr::StaticMethodCall { class, method } => {
                let method_name = method.as_str();

                let owner_class = if class.starts_with('$') {
                    // Variable holding a class-string (e.g. `$cls::make()`).
                    // May resolve to multiple classes for union class-strings.
                    let all_owners: Vec<Arc<ClassInfo>> = ResolvedType::into_arced_classes(
                        crate::completion::resolver::resolve_target_classes(
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
                            let template_subs = Self::build_method_template_subs(
                                owner,
                                method_name,
                                &arg_refs,
                                ctx,
                            );
                            let var_resolver = build_var_resolver(ctx);
                            let mr_ctx = MethodReturnCtx {
                                all_classes: ctx.all_classes,
                                class_loader: ctx.class_loader,
                                template_subs: &template_subs,
                                var_resolver: Some(&var_resolver),
                                cache: ctx.resolved_class_cache,
                                calling_class_name: ctx.current_class.map(|c| c.name.as_str()),
                                is_static: true,
                            };
                            let results = Self::resolve_method_return_types_with_args(
                                owner,
                                method_name,
                                text_args,
                                &mr_ctx,
                            );
                            for r in results {
                                if !union_results.iter().any(|existing| existing.name == r.name) {
                                    union_results.push(r);
                                }
                            }
                        }
                        if !union_results.is_empty() {
                            return union_results;
                        }
                    }
                    all_owners.into_iter().next()
                } else {
                    crate::completion::resolver::resolve_static_owner_class(class, ctx)
                };

                if let Some(ref owner) = owner_class {
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

                    // Capture return type hint for static method calls.
                    if let Some(ref mut hint_out) = return_type_hint_out
                        && let Some(m) = merged.get_method_ci(method_name)
                        && let Some(ref ret) = m.return_type
                    {
                        // Resolve self/static/parent keywords to
                        // concrete class names (mirrors instance path).
                        let resolved_hint = if ret.is_parent_ref() {
                            merged
                                .parent_class
                                .as_ref()
                                .map(|p| PhpType::Named(p.to_string()))
                                .unwrap_or_else(|| ret.clone())
                        } else if ret.is_self_like() {
                            PhpType::Named(merged.fqn().to_string())
                        } else {
                            ret.clone()
                        };
                        **hint_out = Some(resolved_hint);
                    }

                    let split_args = split_text_args(text_args);
                    let arg_refs = split_args.to_vec();
                    let template_subs =
                        Self::build_method_template_subs(&merged, method_name, &arg_refs, ctx);
                    let var_resolver = build_var_resolver(ctx);
                    let mr_ctx = MethodReturnCtx {
                        all_classes: ctx.all_classes,
                        class_loader: ctx.class_loader,
                        template_subs: &template_subs,
                        var_resolver: Some(&var_resolver),
                        cache: ctx.resolved_class_cache,
                        calling_class_name: ctx.current_class.map(|c| c.name.as_str()),
                        is_static: true,
                    };
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

            // ── Standalone function call: app(…) / myHelper(…) ──────
            SubjectExpr::FunctionCall(func_name) => {
                let func_name = func_name.as_str();

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
                // but it mirrors Larastan's `NowAndTodayExtension`, which the
                // ecosystem is written against.  See the matching note in
                // `rhs_resolution.rs`.
                if matches!(
                    normalized_func,
                    "now" | "today" | "Illuminate\\Support\\now" | "Illuminate\\Support\\today"
                ) && let Some(cls) =
                    (ctx.class_loader)(crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)
                {
                    return vec![cls];
                }

                // Check for array element/preserving functions first.
                let is_array_element_func = ARRAY_ELEMENT_FUNCS
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(func_name));
                let is_array_preserving_func = ARRAY_PRESERVING_FUNCS
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(func_name));

                if (is_array_element_func || is_array_preserving_func)
                    && !text_args.is_empty()
                    && let Some(first_arg) = Self::extract_first_arg_text(text_args)
                {
                    let arg_raw_type = Self::resolve_inline_arg_raw_type(&first_arg, ctx);

                    if let Some(ref raw) = arg_raw_type
                        && let Some(element_type) = raw.extract_value_type(true)
                    {
                        let owner_name = ctx.current_class.map(|c| c.name.as_str()).unwrap_or("");
                        let classes: Vec<Arc<ClassInfo>> =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                element_type,
                                owner_name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !classes.is_empty() {
                            return classes;
                        }
                    }
                }

                // Regular function lookup.
                if let Some(fl) = ctx.function_loader
                    && let Some(func_info) = fl(func_name, 0)
                {
                    if let Some(ref cond) = func_info.conditional_return {
                        let var_resolver = build_var_resolver(ctx);
                        let tpl = TemplateContext::with_params(&func_info.template_params);
                        let resolved_type = if !text_args.is_empty() {
                            resolve_conditional_with_text_args(
                                cond,
                                &func_info.parameters,
                                text_args,
                                Some(&var_resolver),
                                ctx.current_class.map(|c| c.name.as_str()),
                                ctx.class_loader,
                                &tpl,
                            )
                        } else {
                            resolve_conditional_without_args(cond, &func_info.parameters)
                        };
                        if let Some(ref parsed_ty) = resolved_type {
                            let classes: Vec<Arc<ClassInfo>> =
                                crate::completion::type_resolution::type_hint_to_classes_typed(
                                    parsed_ty,
                                    "",
                                    ctx.all_classes,
                                    ctx.class_loader,
                                );
                            if !classes.is_empty() {
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
                        let subs = crate::completion::variable::rhs_resolution::build_function_template_subs(
                            &func_info,
                            &split_args,
                            ctx,
                        );

                        if !subs.is_empty()
                            && let Some(ref ret) = func_info.return_type
                        {
                            let substituted = ret.substitute(&subs);
                            let classes: Vec<Arc<ClassInfo>> =
                                crate::completion::type_resolution::type_hint_to_classes_typed(
                                    &substituted,
                                    "",
                                    ctx.all_classes,
                                    ctx.class_loader,
                                );
                            if !classes.is_empty() {
                                return classes;
                            }
                        }
                    }

                    if let Some(ref ret) = func_info.return_type {
                        // Capture the function's return type hint.
                        if let Some(ref mut hint_out) = return_type_hint_out {
                            **hint_out = Some(ret.clone());
                        }
                        return crate::completion::type_resolution::type_hint_to_classes_typed(
                            ret,
                            "",
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    }
                }

                vec![]
            }

            // ── Variable invocation: $fn(…) ─────────────────────────
            SubjectExpr::Variable(var_name) => {
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
                        crate::completion::type_resolution::type_hint_to_classes_typed(
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
                //    inferred as a `PhpType::Callable` (see
                //    `infer_closure_literal_type`), so `$fn`'s embedded
                //    return type covers `$fn = function(): T {}`,
                //    `$fn = fn(): T => …`, and `$fn = strlen(...)` /
                //    `$fn = $obj->method(...)` alike.
                let resolved_var_types = crate::completion::resolver::resolve_target_classes(
                    var_name,
                    AccessKind::Arrow,
                    ctx,
                );
                for rt in &resolved_var_types {
                    if let Some(ret_type) = rt.type_string.callable_return_type() {
                        let classes: Vec<Arc<ClassInfo>> =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
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
                            crate::completion::type_resolution::type_hint_to_classes_typed(
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

            // ── Constructor call: new ClassName(…) ──────────────────
            // A `NewExpr` callee means the call is `new Foo(…)` — the
            // return type is always the class itself.  When the class
            // has `@template` params and the constructor binds them,
            // infer concrete types from `text_args` and apply the
            // substitution so that chained method calls like
            // `(new C("foo"))->get()` propagate generics correctly.
            SubjectExpr::NewExpr { class_name } => {
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
                    let arg_texts: Vec<String> =
                        crate::completion::conditional_resolution::split_text_args(text_args)
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect();
                    if !arg_texts.is_empty() {
                        let mut subs = std::collections::HashMap::new();
                        for (tpl_name, param_name) in &ctor.template_bindings {
                            let param_idx = match ctor
                                .parameters
                                .iter()
                                .position(|p| p.name == param_name.as_str())
                            {
                                Some(idx) => idx,
                                None => continue,
                            };
                            let arg_text = match arg_texts.get(param_idx) {
                                Some(text) => text.trim(),
                                None => continue,
                            };
                            let param_hint = ctor
                                .parameters
                                .get(param_idx)
                                .and_then(|p| p.type_hint.as_ref());
                            let binding_mode =
                                crate::completion::variable::rhs_resolution::classify_template_binding(
                                    tpl_name, param_hint,
                                );
                            use crate::completion::variable::rhs_resolution::TemplateBindingMode;
                            match binding_mode {
                                TemplateBindingMode::Direct => {
                                    if let Some(resolved_type) =
                                        Backend::resolve_arg_text_to_type(arg_text, ctx)
                                    {
                                        crate::completion::variable::rhs_resolution::insert_or_union(&mut subs, tpl_name.to_string(), resolved_type);
                                    }
                                }
                                TemplateBindingMode::ClassStringInner => {
                                    if let Some(resolved_type) =
                                        Backend::resolve_arg_text_to_type(arg_text, ctx)
                                    {
                                        let unwrapped = match resolved_type {
                                            PhpType::ClassString(Some(inner)) => *inner,
                                            _ => resolved_type,
                                        };
                                        crate::completion::variable::rhs_resolution::insert_or_union(&mut subs, tpl_name.to_string(), unwrapped);
                                    }
                                }
                                TemplateBindingMode::ArrayElement => {
                                    if arg_text.starts_with('[') && arg_text.ends_with(']') {
                                        let inner = arg_text[1..arg_text.len() - 1].trim();
                                        if !inner.is_empty() {
                                            let elems =
                                                crate::completion::conditional_resolution::split_text_args(inner);
                                            if let Some(elem) = elems.first()
                                                && let Some(resolved_type) =
                                                    Backend::resolve_arg_text_to_type(
                                                        elem.trim(),
                                                        ctx,
                                                    )
                                            {
                                                crate::completion::variable::rhs_resolution::insert_or_union(
                                                    &mut subs,
                                                    tpl_name.to_string(),
                                                    resolved_type,
                                                );
                                            }
                                        }
                                    } else if let Some(resolved_type) =
                                        Backend::resolve_arg_text_to_type(arg_text, ctx)
                                    {
                                        // Extract the element type from array-like types
                                        // so we bind T to the element, not the whole array.
                                        if let Some(elem_type) = resolved_type.extract_value_type(false) {
                                            crate::completion::variable::rhs_resolution::insert_or_union(&mut subs, tpl_name.to_string(), elem_type.clone());
                                        } else {
                                            crate::completion::variable::rhs_resolution::insert_or_union(&mut subs, tpl_name.to_string(), resolved_type);
                                        }
                                    }
                                }
                                TemplateBindingMode::CallableReturnType => {
                                    // Infer from annotation, generator yields,
                                    // or the unannotated closure's body.
                                    let ret_type =
                                        Backend::infer_closure_return_type(arg_text, ctx);
                                    if let Some(ret_type) = ret_type {
                                        crate::completion::variable::rhs_resolution::insert_or_union(&mut subs, tpl_name.to_string(), ret_type);
                                    }
                                }
                                TemplateBindingMode::CallableParamType(position) => {
                                    if let Some(param_type) =
                                        crate::completion::source::helpers::extract_closure_param_type_from_text(
                                            arg_text, position,
                                        )
                                    {
                                        crate::completion::variable::rhs_resolution::insert_or_union(&mut subs, tpl_name.to_string(), param_type);
                                    }
                                }
                                TemplateBindingMode::GenericWrapper(_, _) => {
                                    // GenericWrapper requires VarResolutionCtx which
                                    // is not available here.  Skip for now — this is
                                    // a rare edge case in chained instantiation.
                                }
                            }
                        }

                        // Remap inherited constructor subs to the child's
                        // template param names via the @extends chain.
                        let effective_subs = if ctor_inherited && !subs.is_empty() {
                            crate::completion::variable::rhs_resolution::remap_inherited_ctor_subs(
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
                            let substituted =
                                crate::virtual_members::resolve_class_fully_with_type_args(
                                    &cls_arc,
                                    ctx.class_loader,
                                    ctx.resolved_class_cache,
                                    &type_args,
                                );
                            if let Some(ref mut hint_out) = return_type_hint_out {
                                **hint_out =
                                    Some(PhpType::Generic(substituted.name.to_string(), type_args));
                            }
                            return vec![substituted];
                        }
                    }
                }

                // Fallback: resolve unbound template params to bounds.
                let type_args = crate::inheritance::default_type_args(&cls_arc);
                let substituted = crate::virtual_members::resolve_class_fully_with_type_args(
                    &cls_arc,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                    &type_args,
                );
                if let Some(ref mut hint_out) = return_type_hint_out {
                    **hint_out = Some(PhpType::Generic(substituted.name.to_string(), type_args));
                }
                vec![substituted]
            }

            // ── Any other callee form (e.g. a nested CallExpr used as
            //    a callee, a PropertyChain for `($this->prop)()`, or a
            //    ClassName that SubjectExpr::parse couldn't distinguish
            //    from a function name) ───────────────────────────────
            _ => {
                // Resolve the callee expression to class(es).
                let callee_classes = ResolvedType::into_arced_classes(
                    crate::completion::resolver::resolve_target_classes_expr(
                        callee,
                        AccessKind::Arrow,
                        ctx,
                    ),
                );

                // When the callee resolves to an object with __invoke(),
                // the call returns __invoke()'s return type, not the
                // object itself.  This handles `($this->formatter)()`.
                for owner in &callee_classes {
                    if let Some(invoke) = owner.get_method("__invoke")
                        && let Some(ref ret) = invoke.return_type
                    {
                        let classes: Vec<Arc<ClassInfo>> =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
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
    }

    /// Resolve a method call's return type, taking into account PHPStan
    /// conditional return types when `text_args` is provided, and
    /// method-level `@template` substitutions when `template_subs` is
    /// non-empty.
    ///
    /// This is the workhorse behind both `resolve_method_return_types`
    /// (which passes `""`) and the inline call-chain path (which passes
    /// the raw argument text from the source, e.g. `"CurrentCart::class"`).
    pub(crate) fn resolve_method_return_types_with_args(
        class_info: &ClassInfo,
        method_name: &str,
        text_args: &str,
        mr_ctx: &MethodReturnCtx<'_>,
    ) -> Vec<Arc<ClassInfo>> {
        let all_classes = mr_ctx.all_classes;
        let class_loader = mr_ctx.class_loader;
        let template_subs = mr_ctx.template_subs;
        let var_resolver = mr_ctx.var_resolver;
        // Helper: try to resolve a method's conditional return type, falling
        // back to template-substituted return type, then plain return type.
        let resolve_method = |method: &MethodInfo| -> Vec<Arc<ClassInfo>> {
            // Try conditional return type first (PHPStan syntax)
            if let Some(ref cond) = method.conditional_return {
                let tpl = TemplateContext {
                    defaults: Some(
                        &class_info
                            .template_param_defaults
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.clone()))
                            .collect::<HashMap<String, PhpType>>(),
                    ),
                    params: &method.template_params,
                    arg_type_resolver: None,
                };
                let resolved_type = if !text_args.is_empty() {
                    resolve_conditional_with_text_args_and_defaults(
                        cond,
                        &method.parameters,
                        text_args,
                        var_resolver,
                        mr_ctx.calling_class_name,
                        mr_ctx.class_loader,
                        &tpl,
                    )
                } else {
                    resolve_conditional_without_args_and_defaults(
                        cond,
                        &method.parameters,
                        tpl.defaults,
                    )
                };
                if let Some(ref parsed) = resolved_type {
                    // Apply method-level template substitutions to the
                    // resolved conditional type (e.g. `TModel` → concrete
                    // class when TModel is a method-level @template param).
                    let effective = if !template_subs.is_empty() {
                        parsed.substitute(template_subs)
                    } else {
                        parsed.clone()
                    };
                    let classes: Vec<Arc<ClassInfo>> =
                        crate::completion::type_resolution::type_hint_to_classes_typed(
                            &effective,
                            &class_info.fqn(),
                            all_classes,
                            class_loader,
                        );
                    if !classes.is_empty() {
                        return classes;
                    }
                }
            }

            // Try method-level @template substitution on the return type.
            // This handles the general case where the return type references
            // a template param (e.g. `@return Collection<T>`) and we have
            // resolved bindings from the call-site arguments.
            if !template_subs.is_empty()
                && let Some(ref ret) = method.return_type
            {
                let substituted = ret.substitute(template_subs);
                if &substituted != ret {
                    let classes: Vec<Arc<ClassInfo>> =
                        crate::completion::type_resolution::type_hint_to_classes_typed(
                            &substituted,
                            &class_info.fqn(),
                            all_classes,
                            class_loader,
                        );
                    if !classes.is_empty() {
                        return classes;
                    }
                }
            }

            // Fall back to plain return type
            if let Some(ref ret) = method.return_type {
                // When the return type is `parent`, resolve to the actual
                // parent class rather than returning the owning class.
                if ret.is_parent_ref() {
                    if let Some(ref parent_name) = class_info.parent_class {
                        let classes =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &PhpType::Named(parent_name.to_string()),
                                &class_info.fqn(),
                                all_classes,
                                class_loader,
                            );
                        if !classes.is_empty() {
                            return classes;
                        }
                    }
                    return vec![];
                }
                // When the return type is `static`, `self`, or `$this`,
                // return the owning class directly.  This avoids a lookup
                // by short name (e.g. "Builder") which fails when the
                // class was loaded cross-file and the short name is not
                // in the current file's use-map or local classes.
                // Returning class_info preserves any generic substitutions
                // already applied (e.g. Builder<User> stays Builder<User>).
                // Match bare `self`/`static`/`$this` as well as nullable
                // (`?static`) and union (`static|null`) forms, plus
                // generic wrappers like `self<RuleError>`, `static<T>`.
                if ret.is_self_like() {
                    return vec![Arc::new(class_info.clone())];
                }
                return crate::completion::type_resolution::type_hint_to_classes_typed(
                    ret,
                    &class_info.fqn(),
                    all_classes,
                    class_loader,
                );
            }
            // Try body return type inference as a last resort.
            // Only for real (non-virtual, non-stub) methods that genuinely
            // lack a return type declaration and docblock @return tag.
            if method.name_offset != 0
                && !method.is_virtual
                && let Some(inferred) = try_infer_body_return_type(&class_info.fqn(), method)
            {
                // A body-inferred `return $this` yields a self-like marker.
                // Map it to the receiver class so the chain continues with
                // the class the method was called on, not the trait/parent
                // that declares the fluent method.
                if inferred.is_self_like() {
                    return vec![Arc::new(class_info.clone())];
                }
                return crate::completion::type_resolution::type_hint_to_classes_typed(
                    &inferred,
                    &class_info.fqn(),
                    all_classes,
                    class_loader,
                );
            }

            vec![]
        };

        // Determine which magic method handles unknown calls for this
        // access kind: `__call` for instance calls, `__callStatic` for
        // static calls.
        let magic_name = if mr_ctx.is_static {
            "__callStatic"
        } else {
            "__call"
        };

        // First check the class itself. Skip this fast path when the
        // declared return type is self-like: a Laravel/Mockery patch may
        // rewrite a bare `self`/`static`/`$this` return to a different
        // concrete type (e.g. `Mockery\LegacyMockInterface::shouldHaveReceived()`
        // really returns `Mockery\VerificationDirector`), and patches are
        // only applied during the merged resolution below. Trusting the
        // raw declaration here would bypass the patch entirely.
        if let Some(method) = class_info.get_method(method_name)
            && !method
                .return_type
                .as_ref()
                .is_some_and(PhpType::is_self_like)
        {
            let result = resolve_method(method);
            if !result.is_empty() {
                return result;
            }
            // Fall through to the merged class — the method may lack a
            // return type here but have one filled in from an interface
            // via `@implements` generic resolution.
        }

        // Walk up the inheritance chain (also merges interface members
        // with `@implements` generic substitutions applied).
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            class_info,
            class_loader,
            mr_ctx.cache,
        );

        // Look up the magic method once; used for both validation and
        // fallback below.
        let magic_method = merged.get_method_ci(magic_name);

        if let Some(method) = merged.get_method(method_name) {
            if method.is_virtual {
                // ── Virtual method (from @method, @mixin, etc.) ─────
                // At runtime these are dispatched through __call /
                // __callStatic.  Validate the virtual method's return
                // type against the magic method's native return type
                // the same way we validate a concrete implementation
                // against an interface: the virtual type can only
                // *narrow* the native constraint, not contradict it.
                if let Some(ref virtual_ret) = method.return_type {
                    if let Some(magic) = magic_method {
                        if let Some(ref native_ret) = magic.native_return_type {
                            // The magic method has a native PHP type
                            // hint.  Check whether the virtual
                            // method's declared type is a valid
                            // narrowing of that native constraint.
                            if is_valid_virtual_narrowing(
                                virtual_ret,
                                native_ret,
                                class_info,
                                all_classes,
                                class_loader,
                            ) {
                                // Valid narrowing — trust the virtual
                                // method's declared type.
                                let result = resolve_method(method);
                                if !result.is_empty() {
                                    return result;
                                }
                            }
                            // Invalid narrowing (lie) or the virtual
                            // type failed to resolve.  Fall through
                            // to the magic-method fallback below,
                            // which will use __call's own return type.
                        } else {
                            // Magic method has no native type hint —
                            // trust the virtual method's declared type.
                            let result = resolve_method(method);
                            if !result.is_empty() {
                                return result;
                            }
                        }
                    } else {
                        // No magic method at all — trust the virtual
                        // method's declared type unconditionally.
                        let result = resolve_method(method);
                        if !result.is_empty() {
                            return result;
                        }
                    }
                }
                // Virtual method with no return type (or whose type
                // was rejected by the validation above).  Fall through
                // to the magic-method fallback below.
            } else {
                // ── Real method ─────────────────────────────────────
                // Real methods are invoked directly at runtime, never
                // through __call.  Use whatever resolve_method
                // returns, even if empty.
                return resolve_method(method);
            }
        }

        // ── Magic-method fallback ───────────────────────────────
        // Either the method was not found at all, or it was a virtual
        // method whose return type was absent or rejected by the
        // native-type validation.  Use the magic method's effective
        // return type (docblock-overridden if available, otherwise
        // native).  When the magic method returns `$this`/`static`/
        // `self`, this preserves the chain type (e.g. Builder<User>
        // stays Builder<User> through dynamic `where{Column}` calls).
        // When it returns `mixed`, no classes resolve and the caller
        // gets an empty vec — the same as before this fallback.
        if let Some(magic) = magic_method {
            let result = resolve_method(magic);
            if !result.is_empty() {
                return result;
            }
        }

        vec![]
    }
}

/// Check whether a virtual method's return type is a valid narrowing of a
/// magic method's (`__call` / `__callStatic`) native return type.
///
/// At runtime, calls to virtual methods (from `@method` tags, `@mixin`
/// members, etc.) are dispatched through the magic method.  The magic
/// method's native PHP type hint is the runtime truth: the virtual
/// method's declared type can only *narrow* it (provide a more specific
/// subtype), not contradict it.
///
/// Returns `true` when the virtual type should be trusted, `false` when
/// it should be rejected in favour of the magic method's type.
///
/// # Examples
///
/// | `__call` native | `@method` type | Result |
/// |-----------------|----------------|--------|
/// | `mixed`         | `Frog`         | ✓ (anything narrows mixed) |
/// | `object`        | `Frog`         | ✓ (any class narrows object) |
/// | `static`        | `ChildClass`   | ✓ if ChildClass extends the owner |
/// | `Animal`        | `Dog`          | ✓ if Dog extends Animal |
/// | `Cement`        | `Frog`         | ✗ (unrelated classes) |
/// | `static`        | `Frog`         | ✗ if Frog does not extend the owner |
/// | `int`           | `string`       | ✗ (incompatible scalars) |
fn is_valid_virtual_narrowing(
    virtual_type: &PhpType,
    native_type: &PhpType,
    owner_class: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    // `mixed` and `void` impose no constraint — any type is valid.
    if native_type.is_mixed() || native_type.is_void() {
        return true;
    }

    // `object` — any class type is a valid narrowing.
    if native_type.is_object() {
        // Only reject if the virtual type is a non-object scalar.
        return !virtual_type.is_scalar();
    }

    // Self-like types (`static`, `self`, `$this`) resolve to the owner
    // class at runtime.  The virtual type must be the owner class itself
    // or a subclass of it.
    if native_type.is_self_like() {
        return is_type_subclass_of(virtual_type, &owner_class.fqn(), all_classes, class_loader);
    }

    // Both are concrete types.  For scalar-to-scalar, delegate to the
    // existing `should_override_type` check which handles compatible
    // refinements (e.g. `string` → `class-string<T>`).
    if native_type.is_scalar() {
        return crate::docblock::should_override_type_typed(virtual_type, native_type);
    }

    // Native is a class type — the virtual type must be the same class
    // or a subclass.
    if let Some(name) = native_type.base_name() {
        is_type_subclass_of(virtual_type, name, all_classes, class_loader)
    } else {
        false
    }
}

/// Check whether `candidate_type` is the same class as `ancestor_name` or
/// a subclass of it, by walking the parent chain.
///
/// Returns `true` when:
/// - The candidate type's base name matches `ancestor_name` (case-insensitive).
/// - The candidate class's parent chain includes `ancestor_name`.
/// - The candidate class cannot be resolved (benefit of the doubt).
fn is_type_subclass_of(
    candidate_type: &PhpType,
    ancestor_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    // Cannot extract a base name → not a class type → not a subclass.
    if candidate_type.base_name().is_none() {
        return false;
    }

    // Build a combined loader that checks local classes first.
    let combined_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        find_class_by_name(all_classes, name)
            .cloned()
            .or_else(|| class_loader(name))
    };

    // Check if the candidate can be resolved at all.  When it cannot,
    // give the benefit of the doubt (e.g. trust an @method tag).
    if let Some(base) = candidate_type.base_name()
        && combined_loader(base).is_none()
    {
        return true;
    }

    crate::util::is_subtype_of_named(candidate_type, ancestor_name, &combined_loader)
}

/// Resolve an arbitrary expression to a [`PhpType`].
///
/// Delegates to [`crate::completion::resolver::resolve_target_classes`] which
/// handles all expression patterns (variables, property chains,
/// method calls, static accesses, etc.) and preserves scalar types
/// through the `type_string` field of [`ResolvedType`].
///
/// When the expression resolves to multiple types (e.g. a variable
/// declared `class-string<A|B>`), all of them are joined into a union
/// so template binding sees the full type rather than only the first
/// member.
pub(super) fn resolve_expression_to_type(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let results = crate::completion::resolver::resolve_target_classes(
        text,
        crate::types::AccessKind::Arrow,
        ctx,
    );
    if results.is_empty() {
        return None;
    }
    Some(crate::types::ResolvedType::types_joined(&results))
}

/// Resolve a method chain by looking up the *declared* return type of the
/// last method call, rather than flattening the whole chain to a bare class
/// name.
///
/// For `$this->transform(str(...))`, this:
///   1. Parses into `CallExpr { callee: MethodCall { base: This, method: "transform" } }`
///   2. Resolves `This` → `Collection` class
///   3. Looks up `transform` on `Collection` → gets declared return type (`$this`)
///   4. Returns `$this` directly, preserving generics and self-references
///
/// Falls back to `None` when the expression is not a method call or the
/// method's return type is unknown.
pub(super) fn resolve_chain_declared_return(
    text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let expr = crate::subject_expr::SubjectExpr::parse(text);
    let (base, method_name) = match &expr {
        crate::subject_expr::SubjectExpr::CallExpr { callee, .. } => match callee.as_ref() {
            crate::subject_expr::SubjectExpr::MethodCall { base, method } => {
                (base.as_ref(), method.as_str())
            }
            _ => return None,
        },
        _ => return None,
    };

    let base_results = crate::completion::resolver::resolve_target_classes_expr(
        base,
        crate::types::AccessKind::Arrow,
        ctx,
    );

    for rt in &base_results {
        let Some(ci) = rt.class_info.as_ref() else {
            continue;
        };

        // Try the raw class first — its return types preserve template
        // parameter names (e.g. `TValue`) that full resolution replaces
        // with their bounds (`mixed`).
        if let Some(method) = ci
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(method_name))
            && let Some(ref ret) = method.return_type
        {
            return Some(ret.clone());
        }

        // Fall back to the fully resolved class for inherited methods.
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            ci,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        if let Some(method) = resolved
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(method_name))
            && let Some(ref ret) = method.return_type
        {
            return Some(ret.clone());
        }
    }

    None
}

/// Resolve a `ClassName::Member` expression to a type.
///
/// Handles enum cases (`MyEnum::Case` → `MyEnum`) and class constants
/// (`Foo::BAR` → the constant's type hint, or the type inferred from
/// the constant's initializer value for untyped constants).
pub(super) fn resolve_static_access_type(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let (class_part, _member) = text.split_once("::")?;

    // Only accept identifier-like class names (no `$var::`, no whitespace).
    if class_part.is_empty()
        || class_part.starts_with('$')
        || !class_part
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
    {
        return None;
    }

    // Resolve `self` / `static` / `parent` to the actual class name.
    let class_name = if is_self_or_static(class_part) {
        ctx.current_class?.name.to_string()
    } else if let Some(resolved) = resolve_class_keyword(class_part, ctx.current_class) {
        resolved
    } else {
        class_part.to_string()
    };

    let cls = (ctx.class_loader)(&class_name)?;

    // Enums: any `EnumName::Case` resolves to the enum type itself.
    if cls.kind == ClassLikeKind::Enum {
        return Some(PhpType::Named(cls.fqn().to_string()));
    }

    // Class constants: look up the constant and use its type hint
    // when available.  Fall back to the owning class type (which is
    // conservative but avoids leaving the raw template param name).
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &cls,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    if let Some(constant) = merged.constants.iter().find(|c| c.name == _member) {
        // Typed class constant — use its declared type.
        if let Some(ref hint) = constant.type_hint {
            return Some(hint.clone());
        }
        // Untyped constant — infer the value type from the initializer
        // so template params bind to the constant's value (e.g. `int`)
        // rather than the owning class.
        if let Some(ref val) = constant.value
            && let Some(ty) =
                crate::completion::variable::rhs_resolution::infer_type_from_constant_value(val)
        {
            return Some(ty);
        }
    }

    // Unknown member or untyped constant we can't classify — we can't
    // determine the type, so return None and let the caller skip the
    // diagnostic.
    None
}

/// Resolve a literal expression to its PHP type.
///
/// Returns `Some(PhpType)` for string literals (`"…"`, `'…'`), integer
/// literals (`42`, `-1`), float literals (`3.14`), boolean literals
/// (`true`, `false`), `null`, and array literals (`[…]`).
pub(super) fn resolve_literal_type(text: &str) -> Option<PhpType> {
    // Closure / arrow function literals: fn(...) or function(...)
    if text.starts_with("fn(")
        || text.starts_with("fn (")
        || text.starts_with("function(")
        || text.starts_with("function (")
    {
        return Some(PhpType::Named("Closure".to_string()));
    }

    // String literals: "…" or '…'
    if (text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''))
    {
        return Some(PhpType::Named("string".to_string()));
    }

    // null
    if text.eq_ignore_ascii_case("null") {
        return Some(PhpType::null());
    }

    // Boolean literals — preserve true/false as distinct types so that
    // template argument inference keeps the precise type (e.g. `C<false>`
    // instead of widening to `C<bool>`).
    if text.eq_ignore_ascii_case("true") {
        return Some(PhpType::true_());
    }
    if text.eq_ignore_ascii_case("false") {
        return Some(PhpType::false_());
    }

    // Array literals: [...] or array(...)
    if (text.starts_with('[') && text.ends_with(']'))
        || (text.starts_with("array(") && text.ends_with(')'))
    {
        return Some(PhpType::Named("array".to_string()));
    }

    // Numeric literals — try int first, then float.
    // Strip an optional leading minus for negative literals.
    let numeric = text.strip_prefix('-').unwrap_or(text);
    if !numeric.is_empty()
        && numeric.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        && numeric.bytes().any(|b| b.is_ascii_digit())
    {
        return Some(PhpType::Named("int".to_string()));
    }
    if !numeric.is_empty()
        && numeric
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'_')
        && numeric.bytes().filter(|&b| b == b'.').count() == 1
        && numeric.bytes().any(|b| b.is_ascii_digit())
    {
        return Some(PhpType::Named("float".to_string()));
    }

    None
}

#[cfg(test)]
mod auth_guard_tests {
    use super::{auth_guard_name, first_string_literal_arg, replace_support_carbon_return};
    use crate::Backend;
    use crate::atom::atom;
    use crate::php_type::PhpType;
    use crate::subject_expr::SubjectExpr;
    use crate::test_fixtures::{make_class, make_method};
    use std::sync::Arc;

    #[test]
    fn first_arg_reads_string_literals() {
        assert_eq!(
            first_string_literal_arg("'admin'").as_deref(),
            Some("admin")
        );
        assert_eq!(
            first_string_literal_arg("\"admin\"").as_deref(),
            Some("admin")
        );
        // Extra arguments after the first are ignored.
        assert_eq!(
            first_string_literal_arg("'admin', true").as_deref(),
            Some("admin")
        );
    }

    #[test]
    fn first_arg_rejects_non_literals() {
        assert_eq!(first_string_literal_arg(""), None);
        assert_eq!(first_string_literal_arg("$guard"), None);
        assert_eq!(first_string_literal_arg("GUARD_NAME"), None);
    }

    #[test]
    fn replaces_support_carbon_inside_nullable_union() {
        assert_eq!(
            replace_support_carbon_return(
                &PhpType::parse("Illuminate\\Support\\Carbon|null"),
                "Carbon\\CarbonImmutable",
            ),
            Some(PhpType::parse("Carbon\\CarbonImmutable|null"))
        );
    }

    #[test]
    fn date_factory_instance_return_uses_configured_class() {
        let mut factory = make_class("DateFactory");
        factory.file_namespace = Some(atom("Illuminate\\Support"));
        factory.methods.push(Arc::new(make_method(
            "now",
            Some("Illuminate\\Support\\Carbon"),
        )));
        factory.rebuild_method_index();

        let immutable = Arc::new(make_class("Carbon\\CarbonImmutable"));
        let loader = |name: &str| {
            (name == crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)
                .then(|| Arc::clone(&immutable))
        };
        let (class, ty) = Backend::configured_laravel_date_return(&factory, "now", &loader)
            .expect("DateFactory::now should use the configured class");

        assert_eq!(class.name, atom("Carbon\\CarbonImmutable"));
        assert_eq!(ty, PhpType::parse("Carbon\\CarbonImmutable"));
    }

    #[test]
    fn date_facade_return_preserves_null_when_configured() {
        let mut facade = make_class("Date");
        facade.file_namespace = Some(atom("Illuminate\\Support\\Facades"));
        facade.methods.push(Arc::new(make_method(
            "create",
            Some("Illuminate\\Support\\Carbon|null"),
        )));
        facade.rebuild_method_index();

        let immutable = Arc::new(make_class("Carbon\\CarbonImmutable"));
        let loader = |name: &str| {
            (name == crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)
                .then(|| Arc::clone(&immutable))
        };
        let (_, ty) = Backend::configured_laravel_date_return(&facade, "create", &loader)
            .expect("Date::create should use the configured class");

        assert_eq!(ty, PhpType::parse("Carbon\\CarbonImmutable|null"));
    }

    /// The guard name is recovered from every call-site form.
    #[test]
    fn guard_name_from_receiver_and_args() {
        let cases = [
            // `auth('admin')->user()`
            ("auth('admin')", "", Some("admin")),
            // `Auth::guard('admin')->user()`
            ("Auth::guard('admin')", "", Some("admin")),
            // `auth()->guard('admin')->user()`
            ("auth()->guard('admin')", "", Some("admin")),
            // `$request->user('admin')` — guard is the `user()` argument.
            ("$request", "'admin'", Some("admin")),
            // Default guard: no argument anywhere.
            ("$request", "", None),
            ("auth()", "", None),
            // A dynamic guard argument cannot be pinned down statically.
            ("auth($name)", "", None),
        ];
        for (base_src, user_args, expected) in cases {
            let base = SubjectExpr::parse(base_src);
            assert_eq!(
                auth_guard_name(&base, user_args).as_deref(),
                expected,
                "base = {base_src:?}, user_args = {user_args:?}"
            );
        }
    }
}
