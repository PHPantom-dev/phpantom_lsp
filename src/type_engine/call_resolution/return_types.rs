/// Call return-type resolution: the primary entry point that resolves a
/// structured call expression + argument text to zero or more `ClassInfo`
/// values, plus the auth/date facade helpers and literal/expression-to-type
/// conversions it depends on.
use crate::atom::atom;
use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::find_class_by_name;
use crate::class_lookup::{is_self_or_static, resolve_class_keyword};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::subject_expr::SubjectExpr;
use crate::type_engine::variable::array_func_rules::{
    array_func_element_type, array_func_raw_type,
};
use crate::types::ClassLikeKind;
use crate::types::*;

use crate::type_engine::conditional_resolution::{
    TemplateContext, VarClassStringResolver, resolve_conditional_with_text_args,
    resolve_conditional_with_text_args_and_defaults, resolve_conditional_without_args,
    resolve_conditional_without_args_and_defaults, split_text_args,
};
use crate::type_engine::resolver::ResolutionCtx;

use super::arg_type_resolution::TextArrayFuncArgs;
use super::target_cache::try_infer_body_return_type;

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
    /// Server state for project-wide answers.  See
    /// [`ResolutionCtx::backend`].
    pub backend: Option<&'a Backend>,
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
    /// The types the call site's arguments resolve to, indexed by
    /// declared parameter, for a method whose return type has to be read
    /// off its body.
    ///
    /// Resolving an argument is as expensive as resolving any other
    /// expression and almost every call needs none of it, so this is a
    /// closure the body-inference fallback calls only once it is certain
    /// it is going to read a body.  `None` from a caller that has no
    /// argument AST to resolve.
    pub call_args: CallSiteArgResolver<'a>,
}

impl<'a> MethodReturnCtx<'a> {
    /// The context a chain-link or static call resolves a method return
    /// through, built from the surrounding resolution context.
    ///
    /// `call_args` is left `None`: a link is reached from resolved
    /// receiver types rather than from the call AST, so there is no
    /// argument list here to resolve.
    pub(crate) fn for_call(
        ctx: &'a ResolutionCtx<'a>,
        template_subs: &'a HashMap<String, PhpType>,
        var_resolver: &'a dyn Fn(&str) -> Vec<String>,
        is_static: bool,
    ) -> Self {
        Self {
            all_classes: ctx.all_classes,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            template_subs,
            var_resolver: Some(var_resolver),
            cache: ctx.resolved_class_cache,
            calling_class_name: ctx.current_class.map(|c| c.name.as_str()),
            is_static,
            call_args: None,
        }
    }
}

/// See [`MethodReturnCtx::call_args`].
pub(crate) type CallSiteArgResolver<'a> = Option<&'a dyn Fn() -> Vec<PhpType>>;

/// Build a [`VarClassStringResolver`] closure from a [`ResolutionCtx`].
///
/// The returned closure resolves a variable name (e.g. `"$requestType"`)
/// to the fully-qualified names of the classes it holds as class-string
/// values by delegating to
/// [`resolve_class_string_targets`](crate::type_engine::variable::class_string_resolution::resolve_class_string_targets).
pub(super) fn build_var_resolver<'a>(
    ctx: &'a ResolutionCtx<'a>,
) -> impl Fn(&str) -> Vec<String> + 'a {
    move |var_name: &str| -> Vec<String> {
        if let Some(cc) = ctx.current_class {
            crate::type_engine::variable::class_string_resolution::resolve_class_string_targets(
                var_name,
                cc,
                ctx.all_classes,
                ctx.content,
                ctx.cursor_offset,
                ctx.class_loader,
                ctx.backend,
            )
            .iter()
            .map(|c| c.fqn().to_string())
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
/// * the context carries no `Backend` (the config and class index the
///   traversal needs), or
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
    // Cheap gate first: without the server state there is nothing to
    // refine, so skip the (comparatively expensive) subtype walk below.
    let backend = ctx.backend?;

    // Only intercept `user()` on an actual auth entry point.  Every
    // other class with a `user()` method must resolve normally.
    let is_auth_receiver = owners.iter().any(|rt| {
        rt.class_info.as_ref().is_some_and(|ci| {
            crate::class_lookup::is_subtype_of(
                ci,
                crate::virtual_members::laravel::GUARD_FQN,
                ctx.class_loader,
            ) || crate::class_lookup::is_subtype_of(
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
    let loader = |name: &str| backend.find_or_load_class(name);
    let model_type = crate::virtual_members::laravel::resolve_auth_user_type(
        backend,
        guard.as_deref(),
        &loader,
    )?;

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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

/// The array shape a Laravel `validated()` / `validate()` /
/// `safe()->only()` call returns, given the rules in scope at the call site.
///
/// Returns `None` for every other call, leaving the declared return type
/// alone.
/// The type a request input accessor call returns, given the arguments it
/// was written with.
///
/// The subject-expression path reaches the arguments as text, which is all
/// the key needs; the default's type is resolved through the shared
/// pipeline the same way an argument anywhere else is.
fn resolve_request_accessor_at_call(
    method_name: &str,
    text_args: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    use crate::virtual_members::laravel::request_input;

    let accessor = request_input::input_accessor(method_name)?;
    let receiver = owners.iter().find_map(|rt| rt.class_info.as_ref())?;
    // The accessor is declared on `Illuminate\Http\Request`, while the
    // receiver is usually an app's own `FormRequest` subclass that never
    // redeclares it, so its parameters have to be found by walking the
    // parent chain rather than reading `receiver`'s own members.
    let (method, _) = crate::type_engine::types::narrowing::find_method_in_chain_where(
        receiver,
        method_name,
        ctx.class_loader,
        &|_| true,
        &mut Vec::new(),
        0,
    )?;
    let args = split_text_args(text_args);
    let bound = crate::call_args::bind_text_args_to_params(&method.parameters, &args);
    let default_type = || {
        let text = bound.get(1)?.as_deref()?;
        let resolved =
            crate::type_engine::resolver::resolve_target_classes(text, AccessKind::Arrow, ctx);
        (!resolved.is_empty()).then(|| ResolvedType::types_joined(&resolved))
    };
    request_input::resolve_accessor_type(
        receiver,
        accessor,
        &request_input::AccessorArgs {
            key: bound.first().and_then(|k| k.as_deref()),
            default_type: &default_type,
        },
        ctx.content,
        ctx.cursor_offset,
        ctx.class_loader,
        ctx.backend,
    )
}

fn resolve_validated_shape_at_call(
    base: &SubjectExpr,
    method_name: &str,
    text_args: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    use crate::virtual_members::laravel::validated_shape;

    let call = validated_shape::shape_bearing_method(method_name)?;
    let receiver = owners.iter().find_map(|rt| rt.class_info.as_ref())?;
    let mut args = split_text_args(text_args);
    for arg in &mut args {
        *arg = crate::call_args::text_arg_value(arg);
    }

    validated_shape::resolve_shape_at_call(
        receiver,
        call,
        &args,
        &|| validated_shape::safe_source_class(base, ctx),
        ctx.content,
        ctx.cursor_offset,
        ctx.class_loader,
        ctx.backend,
    )
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
    crate::text_scan::unquote_php_string(crate::call_args::text_arg_value(first))
        .map(str::to_string)
}

fn replace_support_carbon_return(ty: &PhpType, configured_class: &str) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Named(name) => (name.trim_start_matches('\\')
            == crate::virtual_members::laravel::SUPPORT_CARBON_FQN)
            .then(|| PhpType::named(atom(configured_class))),
        TypeKind::Nullable(inner) => {
            replace_support_carbon_return(inner, configured_class).map(PhpType::nullable)
        }
        TypeKind::Union(members) => {
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
            replaced.then_some(PhpType::union(members))
        }
        _ => None,
    }
}

/// Resolve a method's PHPStan-style conditional return type (if any)
/// against call-site arguments and template substitutions, returning the
/// winning branch with template substitutions already applied.
///
/// Returns `None` when the method has no conditional return type, or when
/// the condition cannot be decided from the arguments — callers fall back
/// to the method's plain `return_type` in that case.  Shared by
/// [`Backend::resolve_method_return_types_with_args`] (which needs the
/// winning branch's classes) and the call-chain hint capture in
/// `resolve_call_return_types_on_receiver_inner` (which needs the winning
/// branch's full type, e.g. to preserve an intersection) so the two agree
/// on what a conditional return type resolves to.
fn resolve_conditional_return_hint(
    method: &MethodInfo,
    text_args: &str,
    var_resolver: VarClassStringResolver<'_>,
    template_subs: &HashMap<String, PhpType>,
    calling_class_name: Option<&str>,
    declaring_fqn: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let cond = method.conditional_return.as_ref()?;
    let class_values =
        crate::inheritance::class_scoped_template_values(template_subs, &method.template_params);
    let tpl = TemplateContext {
        defaults: Some(class_values.as_ref()),
        params: &method.template_params,
        bindings: &method.template_bindings,
        arg_type_resolver: None,
    };
    let resolved = if !text_args.is_empty() {
        resolve_conditional_with_text_args_and_defaults(
            cond,
            &method.parameters,
            text_args,
            var_resolver,
            crate::type_engine::conditional_resolution::ConditionalClassContext {
                calling: calling_class_name,
                declaring: Some(declaring_fqn),
            },
            class_loader,
            &tpl,
        )
    } else {
        resolve_conditional_without_args_and_defaults(cond, &method.parameters, tpl.defaults)
    }?;
    Some(if !template_subs.is_empty() {
        resolved.substitute(template_subs)
    } else {
        resolved
    })
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
    /// Optionally captures the raw return type hint (with template
    /// substitutions applied) into `return_type_hint_out` when provided.
    /// This preserves generic type parameters (e.g. `HasMany<Translation,
    /// Tag>`) that would otherwise be lost when converting to
    /// `Vec<Arc<ClassInfo>>`.
    pub(crate) fn resolve_call_return_types_expr_with_hint(
        callee: &SubjectExpr,
        text_args: &str,
        ctx: &ResolutionCtx<'_>,
        return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        Self::resolve_call_return_types_on_receiver(
            callee,
            text_args,
            None,
            ctx,
            return_type_hint_out,
        )
    }

    /// [`resolve_call_return_types_expr_with_hint`] with the receiver of an
    /// instance method call optionally already resolved.
    ///
    /// A `Some(receiver)` skips resolving the callee's base, which is how a
    /// fluent chain is walked outward from its base without recursing into
    /// each link (see `resolve_target_classes_expr`).  The base expression is
    /// still needed for the Laravel interceptions that read the receiver's
    /// *syntax* (which guard an `auth()` call names, which request a
    /// validation shape belongs to).
    pub(crate) fn resolve_call_return_types_on_receiver(
        callee: &SubjectExpr,
        text_args: &str,
        receiver: Option<Vec<ResolvedType>>,
        ctx: &ResolutionCtx<'_>,
        mut return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        let classes = Self::resolve_call_return_types_on_receiver_inner(
            callee,
            text_args,
            receiver,
            ctx,
            return_type_hint_out.as_deref_mut(),
        );

        // A `@return value-of<ID_TABLE>` reaches here as the operator the
        // docblock parser could not finish: only the template path reads the
        // constant behind the name, and a plain function never takes it.
        // Finish it on whatever hint came back, so the caller sees the value
        // union rather than a type expression that widens to `mixed`.
        let Some(hint_out) = return_type_hint_out else {
            return classes;
        };
        let Some(evaluated) = hint_out
            .as_ref()
            .and_then(|hint| super::evaluate_constant_operands(hint, ctx))
        else {
            return classes;
        };
        // The operator stood in for the classes the resolution below could
        // not name; now that it has evaluated, they can be named.
        let classes = if classes.is_empty() {
            crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &evaluated,
                "",
                ctx.all_classes,
                ctx.class_loader,
            )
        } else {
            classes
        };
        *hint_out = Some(evaluated);
        classes
    }

    fn resolve_call_return_types_on_receiver_inner(
        callee: &SubjectExpr,
        text_args: &str,
        receiver: Option<Vec<ResolvedType>>,
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
                let lhs_resolved: Vec<ResolvedType> = receiver.unwrap_or_else(|| {
                    crate::type_engine::resolver::resolve_target_classes_expr(
                        base,
                        AccessKind::Arrow,
                        ctx,
                    )
                });

                // A property read through the Reflection API: the name
                // handed to `getProperty()` decides the type, so the
                // stub's `ReflectionProperty` / `mixed` return types are
                // as specific as an annotation can be.
                if super::is_reflected_property_call(method_name)
                    && let Some(ty) = super::resolve_reflected_property_at_call(
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
                    && let Some(classes) =
                        resolve_auth_user_at_call(base, text_args, &lhs_resolved, ctx)
                {
                    return classes;
                }

                // Laravel factory count state: `create()`/`make()` build
                // a single model, or a collection of them when the chain
                // set a count (`factory(3)`, `count(3)`, `times(3)`).
                if let Some((classes, hint)) =
                    crate::virtual_members::laravel::resolve_factory_count_return(
                        base,
                        method_name,
                        &lhs_resolved,
                        ctx,
                    )
                {
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
                if let Some(shape) = resolve_validated_shape_at_call(
                    base,
                    method_name,
                    text_args,
                    &lhs_resolved,
                    ctx,
                ) {
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
                    let method_subs =
                        Self::build_method_template_subs(&owner, method_name, &arg_refs, ctx);

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
                                    let arg_ty_resolver =
                                        |t: &str| Self::resolve_arg_text_to_type(t, ctx);
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
                                        TypeKind::Generic(_) => {
                                            s.replace_self_with_type(&rt.type_string)
                                        }
                                        _ => s.replace_self(&owner.fqn()),
                                    },
                                ));
                            }
                            hint_captured = true;
                        }
                    }
                    let mr_ctx =
                        MethodReturnCtx::for_call(ctx, &template_subs, &var_resolver, false);
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

            // ── Static method call: Class::method(…) ────────────────
            SubjectExpr::StaticMethodCall { class, method } => {
                let method_name = method.as_str();

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
                            let template_subs = Self::build_method_template_subs(
                                owner,
                                method_name,
                                &arg_refs,
                                ctx,
                            );
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
                    let concrete_owner = super::facade_concrete_owner(
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
                    let mr_ctx =
                        MethodReturnCtx::for_call(ctx, &template_subs, &var_resolver, true);
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
                ) && let Some(cls) =
                    (ctx.class_loader)(crate::virtual_members::laravel::VIEW_FQN)
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
                    if crate::type_engine::variable::string_func_rules::is_foldable_string_func(
                        func_name,
                    ) && let Some(folded) =
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
                        || crate::type_engine::types::flag_returns::has_flag_dependent_return(
                            func_name,
                        )
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
                                    return resolve_conditional_without_args(
                                        cond,
                                        &func_info.parameters,
                                    );
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
                            let parsed_ty = crate::type_engine::variable::rhs_resolution::substitute_function_templates(
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
                        let subs = crate::type_engine::variable::rhs_resolution::build_function_template_subs(
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
                let resolved_var_types = crate::type_engine::resolver::resolve_target_classes(
                    var_name,
                    AccessKind::Arrow,
                    ctx,
                );
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

                // `new ReflectionProperty(C::class, 'name')` is the value
                // `ReflectionClass::getProperty('name')` builds, written
                // the other way, so it carries the same class and name.
                if super::is_reflected_property_class(cls_arc.fqn().as_str())
                    && let Some(ty) = super::resolve_reflected_property_at_new(
                        &cls_arc,
                        &split_text_args(text_args),
                        ctx,
                    )
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
                    let arg_texts =
                        crate::type_engine::conditional_resolution::split_text_args(text_args);
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
                            let substituted =
                                crate::virtual_members::resolve_class_fully_with_type_args(
                                    &cls_arc,
                                    ctx.class_loader,
                                    ctx.resolved_class_cache,
                                    &type_args,
                                );
                            if let Some(ref mut hint_out) = return_type_hint_out {
                                **hint_out =
                                    Some(PhpType::generic_atom(substituted.fqn(), type_args));
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

            // ── Any other callee form (e.g. a nested CallExpr used as
            //    a callee, a PropertyChain for `($this->prop)()`, or a
            //    ClassName that SubjectExpr::parse couldn't distinguish
            //    from a function name) ───────────────────────────────
            _ => {
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
        let template_values =
            crate::inheritance::template_values_with_defaults(class_info, mr_ctx.template_subs);
        let template_subs = template_values.as_ref();
        let var_resolver = mr_ctx.var_resolver;
        // Helper: try to resolve a method's conditional return type, falling
        // back to template-substituted return type, then plain return type.
        let resolve_method = |method: &MethodInfo| -> Vec<Arc<ClassInfo>> {
            // Try conditional return type first (PHPStan syntax)
            if let Some(effective) = resolve_conditional_return_hint(
                method,
                text_args,
                var_resolver,
                template_subs,
                mr_ctx.calling_class_name,
                class_info.fqn().as_str(),
                mr_ctx.class_loader,
            ) {
                let classes: Vec<Arc<ClassInfo>> =
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        &effective,
                        &class_info.fqn(),
                        all_classes,
                        class_loader,
                    );
                if !classes.is_empty() {
                    return classes;
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
                        crate::type_engine::type_resolution::type_hint_to_classes_typed(
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

            // Fall back to plain return type.  A `mixed` return (native or
            // docblock) carries no information, so it is treated the same
            // as no declared type at all: skip straight to body inference
            // below rather than resolving it to zero classes here.
            if let Some(ref ret) = method.return_type
                && !ret.is_mixed()
            {
                // When the return type is `parent`, resolve to the actual
                // parent class rather than returning the owning class.
                if ret.is_parent_ref() {
                    if let Some(ref parent_name) = class_info.parent_class {
                        let classes =
                            crate::type_engine::type_resolution::type_hint_to_classes_typed(
                                &PhpType::named(atom(parent_name.as_ref())),
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
                return crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    ret,
                    &class_info.fqn(),
                    all_classes,
                    class_loader,
                );
            }
            // Try body return type inference as a last resort.
            // Only for real (non-virtual, non-stub) methods that genuinely
            // lack a return type declaration and docblock @return tag, or
            // whose only declared type is `mixed`.
            if method.name_offset != 0
                && !method.is_virtual
                && let Some(backend) = mr_ctx.backend
                && let Some(inferred) = try_infer_body_return_type(
                    backend,
                    &class_info.fqn(),
                    method,
                    &mr_ctx
                        .call_args
                        .map(|resolve| resolve())
                        .unwrap_or_default(),
                )
            {
                // A body-inferred `return $this` yields a self-like marker.
                // Map it to the receiver class so the chain continues with
                // the class the method was called on, not the trait/parent
                // that declares the fluent method.
                if inferred.is_self_like() {
                    return vec![Arc::new(class_info.clone())];
                }
                return crate::type_engine::type_resolution::type_hint_to_classes_typed(
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

    crate::class_lookup::is_subtype_of_named(candidate_type, ancestor_name, &combined_loader)
}

/// Resolve an arbitrary expression to a [`PhpType`].
///
/// Delegates to [`crate::type_engine::resolver::resolve_target_classes`] which
/// handles all expression patterns (variables, property chains,
/// method calls, static accesses, etc.) and preserves scalar types
/// through the `type_string` field of [`ResolvedType`].
///
/// When the expression resolves to multiple types (e.g. a variable
/// declared `class-string<A|B>`), all of them are joined into a union
/// so template binding sees the full type rather than only the first
/// member.
pub(super) fn resolve_expression_to_type(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let expr = SubjectExpr::parse(text);
    let results = crate::type_engine::resolver::resolve_target_classes_expr(
        &expr,
        crate::types::AccessKind::Arrow,
        ctx,
    );
    if results.is_empty() {
        return None;
    }
    let walked = crate::types::ResolvedType::types_joined(&results);
    Some(restore_dropped_call_arms(&expr, walked, ctx))
}

/// Put back the alternatives of a call's declared return type that the
/// class walk had no way to report.
///
/// [`resolve_expression_to_type`] answers with the classes an expression
/// can be, so a `?Carbon` return arrives as a bare `Carbon` and a
/// `Carbon|string` one as a bare `Carbon`: neither `null` nor `string`
/// is a class.  A `@template` bound from that argument then claims the
/// value can only ever be the class, which both hides a mismatch where
/// the substituted type is consumed and invents one where a parameter is
/// checked against it.
///
/// Only the alternatives of a union (or of a `?T`) are restored, and only
/// the ones that bottom out in built-in types: a lone `class-string<User>`
/// or `array{user: User}` is *represented* by the class the walk found
/// rather than dropped by it, and re-adding it beside that class would
/// name the same value twice.  `static`, `$this`, and unbound template
/// names are likewise left out, since the walk resolves them on purpose.
///
/// A call that narrowing can key on is skipped entirely: there the walk's
/// answer may be a narrowed type, and the declared return is exactly what
/// the check refined away.
fn restore_dropped_call_arms(
    expr: &SubjectExpr,
    walked: PhpType,
    ctx: &ResolutionCtx<'_>,
) -> PhpType {
    let SubjectExpr::CallExpr { callee, args_text } = expr else {
        return walked;
    };
    if crate::type_engine::resolver::narrowable_call_key(expr).is_some() {
        return walked;
    }

    let mut hint = None;
    Backend::resolve_call_return_types_on_receiver(callee, args_text, None, ctx, Some(&mut hint));
    let Some(hint) = hint else {
        return walked;
    };
    // `raw_kind` rather than `kind`, so a `__benevolent<string|false>` is
    // not mistaken for the union it wraps: the marker says the failure arm
    // is not worth enforcing, which is the opposite of restoring it.
    if !matches!(hint.raw_kind(), TypeKind::Union(_) | TypeKind::Nullable(_)) {
        return walked;
    }

    let mut present = Vec::new();
    collect_top_level_arms(&walked, &mut present);
    let mut arms = Vec::new();
    collect_top_level_arms(&hint, &mut arms);

    let extra: Vec<PhpType> = arms
        .into_iter()
        .filter(|arm| arm.is_scalar_leaf() && !present.contains(arm))
        .collect();
    if extra.is_empty() {
        return walked;
    }
    if extra.len() == 1 && extra[0].is_null() {
        return PhpType::nullable(walked);
    }
    let mut members = present;
    members.extend(extra);
    PhpType::union(members)
}

/// Flatten a type into the alternatives it offers at the top level,
/// spelling a `?T` as its two arms so `null` can be compared like any
/// other member.
fn collect_top_level_arms(ty: &PhpType, out: &mut Vec<PhpType>) {
    match ty.raw_kind() {
        TypeKind::Union(members) => {
            for member in members {
                collect_top_level_arms(member, out);
            }
        }
        TypeKind::Nullable(inner) => {
            collect_top_level_arms(inner, out);
            out.push(PhpType::named(atom("null")));
        }
        _ => out.push(ty.clone()),
    }
}

/// Resolve a call expression to the return type the shared call-resolution
/// path computes for it, whether or not that type is backed by a class.
///
/// [`resolve_expression_to_type`] reports only class-backed results, so a
/// call returning a scalar or an array shape (`getRating(): int`) comes
/// back empty even though the call resolved fine. This reads the same
/// path's return-type hint, which already has class-level and method-level
/// template substitution applied.
///
/// Returns `None` when the text is not a call expression, or when the call
/// resolves to no return type at all.
pub(super) fn resolve_call_return_hint(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let expr = SubjectExpr::parse(text);
    let SubjectExpr::CallExpr { callee, args_text } = &expr else {
        return None;
    };
    let mut hint = None;
    Backend::resolve_call_return_types_on_receiver(callee, args_text, None, ctx, Some(&mut hint));
    hint
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
    let expr = crate::type_engine::subject_expr::SubjectExpr::parse(text);
    let (base, method_name) = match &expr {
        crate::type_engine::subject_expr::SubjectExpr::CallExpr { callee, .. } => {
            match callee.as_ref() {
                crate::type_engine::subject_expr::SubjectExpr::MethodCall { base, method } => {
                    (base.as_ref(), method.as_str())
                }
                _ => return None,
            }
        }
        _ => return None,
    };

    let base_results = crate::type_engine::resolver::resolve_target_classes_expr(
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
pub(crate) fn resolve_static_access_type(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
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
        return Some(PhpType::named(cls.fqn()));
    }

    // Class constants: use the declared type hint when available,
    // otherwise infer a type from the initializer value.
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &cls,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    if let Some(constant) = merged.constants.iter().find(|c| c.name == _member) {
        // Infer the value type from the initializer so template params bind
        // to the constant's value (e.g. `int`) rather than the owning class.
        //
        // A declared type (PHP 8.3's `const int NAME = …`) says what the
        // constant may hold, not what it does hold, so the initialiser is
        // still the sharper answer and is read first. It only stands in for
        // the declaration when it refines it: an initialiser naming an enum
        // case resolves to the case's class, which the structural check
        // rejects, leaving the declared type as before.
        if let Some(ref val) = constant.value {
            let inferred =
                crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(val)
                    .or_else(|| folded_class_constant_type(&merged, _member, val, ctx));
            if let Some(ty) = inferred.filter(|ty| {
                constant
                    .type_hint
                    .as_ref()
                    .is_none_or(|hint| ty.is_subtype_of(hint))
            }) {
                return Some(ty);
            }
        }
        if let Some(ref hint) = constant.type_hint {
            return Some(hint.clone());
        }

        // An untyped constant whose initialiser is itself `Class::Case`
        // holds that case's own enum type — the structural check above
        // deliberately skips it (an enum case is not a `Literal`), and
        // there is no declared type hint to fall back to here. Recurse
        // into this same function on the initialiser text rather than
        // teaching it a second way to read an enum case; guarded by the
        // same re-entrancy key `folded_class_constant_type` folds under,
        // so a constant defined in terms of itself (directly or through
        // another constant) reports unresolvable instead of recursing
        // forever.
        if let Some(ref val) = constant.value {
            let key = format!("{}::{}", merged.fqn(), _member);
            let _guard = crate::type_engine::types::const_fold::FoldGuard::acquire(&key)?;
            let qualified = qualify_class_keyword(val, &merged);
            if let Some(ty) = resolve_static_access_type(&qualified, ctx) {
                return Some(ty);
            }
        }
    }

    // Unknown member or untyped constant we can't classify — we can't
    // determine the type, so return None and let the caller skip the
    // diagnostic.
    None
}

/// The literal value an untyped class constant holds, folded from an
/// initialiser that names other constants (`const FLAGS = JSON_THROW_ON_ERROR;`,
/// `const COMBO = A | B;`).
///
/// `class` is the class the constant was looked up on, with its inherited
/// members merged in, so `self::` inside the initialiser is read against a
/// class that has the constant it names.
pub(crate) fn folded_class_constant_type(
    class: &ClassInfo,
    const_name: &str,
    value: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolve =
        |text: &str| Backend::resolve_arg_text_to_type(&qualify_class_keyword(text, class), ctx);
    let key = format!("{}::{}", class.fqn(), const_name);
    crate::type_engine::types::const_fold::folded_constant_type(&key, value, &resolve)
}

/// The literal value a global constant holds, folded from an initialiser that
/// names other constants (`const FLAGS = JSON_THROW_ON_ERROR;`, `define('MASK',
/// A | B)`).
pub(crate) fn folded_global_constant_type(
    name: &str,
    value: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolve = |text: &str| Backend::resolve_arg_text_to_type(text, ctx);
    crate::type_engine::types::const_fold::folded_constant_type(name, value, &resolve)
}

/// `text` with a leading `self::`/`static::`/`parent::` replaced by the class
/// it names, so a term read out of a constant's initialiser resolves against
/// the class that declared it rather than the one being read from.
fn qualify_class_keyword<'t>(text: &'t str, class: &ClassInfo) -> std::borrow::Cow<'t, str> {
    let (keyword, rest) = match text.split_once("::") {
        Some(parts) => parts,
        None => return std::borrow::Cow::Borrowed(text),
    };
    let qualifier = if is_self_or_static(keyword) {
        class.fqn().to_string()
    } else if let Some(parent) = class
        .parent_class
        .filter(|_| keyword.eq_ignore_ascii_case("parent"))
    {
        parent.to_string()
    } else {
        return std::borrow::Cow::Borrowed(text);
    };
    std::borrow::Cow::Owned(format!("{qualifier}::{rest}"))
}

/// Resolve `self`/`static`/`parent` in a return-type hint to concrete
/// class names, so downstream consumers see real FQNs rather than
/// keywords, then apply the Eloquent-collection patch every hint gets.
///
/// `parent` always resolves to `owner`'s parent (or is left as-is when it
/// has none). `self`/`static` is what genuinely differs between the
/// instance and static call paths, so `replace_self` decides that part:
/// the instance path prefers the receiver's own generic type
/// (`Builder<User>`) so a fluent chain keeps its bound template argument,
/// while a static call replaces only the bare name.
fn resolve_hint_keywords(
    hint: PhpType,
    owner: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    replace_self: impl FnOnce(PhpType) -> PhpType,
) -> PhpType {
    let resolved = if hint.is_parent_ref() {
        owner
            .parent_class
            .as_ref()
            .map(|p| PhpType::named(atom(p.as_ref())))
            .unwrap_or(hint)
    } else if hint.contains_self_ref() {
        replace_self(hint)
    } else {
        hint
    };
    crate::virtual_members::laravel::replace_eloquent_collections_in_type(&resolved, class_loader)
        .unwrap_or(resolved)
}

/// The type a cast expression produces, or `None` when the text is not a
/// cast.
///
/// A cast names its own result whatever its operand turns out to be, which is
/// what makes it readable from the source text alone: `(string) $customer->id`
/// is a `string` without resolving the property. `(array)` promises an array
/// but says nothing about its contents, so it stays bare.
///
/// Only a cast applied to a single operand answers. A cast that is one side of
/// a larger expression (`(int) $a / 2`, `(int) $a === $b`) has the operator's
/// result type, not the cast's, so those are left to the caller's other paths
/// rather than answered wrongly.
pub(super) fn resolve_cast_type(text: &str) -> Option<PhpType> {
    let (keyword, operand) = text.strip_prefix('(')?.split_once(')')?;
    if !operand_is_single(operand.trim()) {
        return None;
    }
    let name = match keyword.trim().to_ascii_lowercase().as_str() {
        "string" | "binary" => "string",
        "int" | "integer" => "int",
        "float" | "double" | "real" => "float",
        "bool" | "boolean" => "bool",
        "array" => "array",
        "object" => "object",
        _ => return None,
    };
    Some(PhpType::named(atom(name)))
}

/// The type an operator expression produces, for the operators whose
/// answer can be read from the source text without a full parse.
///
/// Concatenation (`$a . $b`) always yields `string`, whatever its operands
/// are. The elvis operator (`$body ?: ''`) yields the union of both sides —
/// resolved recursively through [`Backend::resolve_arg_text_to_type`], the
/// same way assigning the expression to a variable first would resolve
/// through the AST-based `resolve_conditional_chain`.
///
/// A full three-part ternary (`$a ? $b : $c`) and arithmetic operators
/// (`+`, `-`, `*`, …) are deliberately left unanswered here: arithmetic's
/// result depends on whether its operands are int or float (and `+` alone
/// can mean array union), which the source text can't decide without
/// resolving both operands' concrete types.
pub(super) fn resolve_operator_type(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    if contains_top_level_concat(text) {
        return Some(PhpType::named(atom("string")));
    }
    // A bitwise expression over constants is the value PHP computes for it
    // (`JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR` is one mask, not two flags).
    // Only an expression that actually has an operator folds: a single term
    // would ask this very resolver about the same text again.
    if crate::type_engine::types::const_fold::has_top_level_bitwise_operator(text) {
        let resolve = |term: &str| Backend::resolve_arg_text_to_type(term, ctx);
        if let Some(value) =
            crate::type_engine::types::const_fold::fold_int_expression(text, &resolve)
        {
            return Some(PhpType::literal_int(value.to_string()));
        }
    }
    // `??` binds looser than `?:`, so it is split first: the left operand of
    // `$a ?? $b ?: $c` is `$a` and the right is the whole ternary.  The
    // coalesce only yields its left operand when that operand is not null,
    // so the `null` arm cannot survive into the result.
    if let Some((left, right)) = split_top_level_coalesce(text) {
        // A left operand that is only ever null contributes nothing, which
        // `non_null_type` reports as `None` — the same answer an unresolvable
        // operand gives, and the right operand carries the result either way.
        let left_ty = Backend::resolve_arg_text_to_type(left, ctx).and_then(|ty| {
            if ty.is_null() {
                None
            } else {
                Some(ty.non_null_type().unwrap_or(ty))
            }
        });
        let right_ty = Backend::resolve_arg_text_to_type(right, ctx);
        return join_operand_types(left_ty, right_ty);
    }

    if let Some((left, right)) = split_top_level_elvis(text) {
        let left_ty = Backend::resolve_arg_text_to_type(left, ctx);
        let right_ty = Backend::resolve_arg_text_to_type(right, ctx);
        return join_operand_types(left_ty, right_ty);
    }
    None
}

/// The type a `??`/`?:` result carries given what each side resolved to.
///
/// Whichever side resolved when only one did is the answer, since the
/// other operand contributed nothing to know about; when both did and
/// they differ, the result could be either, so the answer is their union.
fn join_operand_types(left: Option<PhpType>, right: Option<PhpType>) -> Option<PhpType> {
    match (left, right) {
        (Some(l), Some(r)) if l == r => Some(l),
        (Some(l), Some(r)) => Some(PhpType::union(vec![l, r])),
        (Some(l), None) => Some(l),
        (None, Some(r)) => Some(r),
        (None, None) => None,
    }
}

/// What a top-level scan makes of the byte it is looking at.
enum ScanStep {
    /// Step over this many bytes without interpreting them further.
    Skip(usize),
    /// Stop and report this offset.
    Stop,
    /// Stop and report nothing: what was found rules the whole scan out.
    Abort,
}

/// Walk `text`'s top level, calling `at_depth_zero` on every byte that is
/// not inside a quote or a bracket.
///
/// Quoting (`'…'` and `"…"`, with backslash escapes) and nesting (`(`,
/// `[`, `{`) are handled here, so a scanner only has to say what it makes
/// of the operators it is looking for.
///
/// Returns the offset the visitor stopped at, or `None` when it aborted or
/// the scan ran to the end.
fn scan_top_level(text: &str, at_depth_zero: impl Fn(&[u8], usize) -> ScanStep) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth: u32 = 0;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => {
                quote = Some(b);
                i += 1;
                continue;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }
        if depth > 0 {
            i += 1;
            continue;
        }
        match at_depth_zero(bytes, i) {
            ScanStep::Skip(n) => i += n.max(1),
            ScanStep::Stop => return Some(i),
            ScanStep::Abort => return None,
        }
    }
    None
}

/// Whether `text` joins its parts with a top-level `.` concatenation.
fn contains_top_level_concat(text: &str) -> bool {
    scan_top_level(text, |bytes, i| match bytes[i] {
        b'?' if bytes[i..].starts_with(b"?->") => ScanStep::Skip(3),
        b'-' if bytes[i..].starts_with(b"->") => ScanStep::Skip(2),
        // `...` (spread/variadic) is not concatenation.
        b'.' if bytes[i..].starts_with(b"...") => ScanStep::Skip(3),
        b'.' => ScanStep::Stop,
        _ => ScanStep::Skip(1),
    })
    .is_some()
}

/// Split `text` at the first top-level null-coalescing operator (`??`).
///
/// `??` is right-associative, so splitting at the *first* one leaves the
/// rest of a `$a ?? $b ?? $c` chain in the right operand for the caller to
/// resolve the same way. The assignment form `??=` is not an expression
/// operator and is left alone.
fn split_top_level_coalesce(text: &str) -> Option<(&str, &str)> {
    let at = scan_top_level(text, |bytes, i| {
        if !bytes[i..].starts_with(b"??") {
            ScanStep::Skip(1)
        } else if bytes[i..].starts_with(b"??=") {
            ScanStep::Abort
        } else {
            ScanStep::Stop
        }
    })?;
    let left = text[..at].trim();
    let right = text[at + 2..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

/// Split `text` at a top-level elvis operator (`?:`), stepping over the
/// nullsafe `?->` (which is not this).
///
/// Returns the trimmed left and right operand texts, or `None` when no
/// top-level `?:` is found — including a full ternary (`$a ? $b : $c`),
/// which is left to the caller's other paths.
fn split_top_level_elvis(text: &str) -> Option<(&str, &str)> {
    let at = scan_top_level(text, |bytes, i| {
        if bytes[i] == b'?'
            && !bytes[i..].starts_with(b"?->")
            && text[i + 1..].trim_start().starts_with(':')
        {
            ScanStep::Stop
        } else {
            ScanStep::Skip(1)
        }
    })?;
    let rest = text[at + 1..].trim_start().strip_prefix(':')?;
    Some((text[..at].trim_end(), rest.trim_start()))
}

/// Whether `operand` is one expression rather than several joined by an
/// operator.
///
/// A variable, property or method chain, array index, call, or literal counts
/// as one; anything carrying a binary operator at the top level (outside its
/// own brackets, quotes and parentheses) does not. `->` and `?->` are chain
/// links, not operators.
fn operand_is_single(operand: &str) -> bool {
    if operand.is_empty() {
        return false;
    }
    scan_top_level(operand, |bytes, i| match bytes[i] {
        // `->` and `?->` continue the chain, so they are stepped over
        // whole; a bare `-` or `?` is subtraction or a ternary.
        b'-' if bytes[i..].starts_with(b"->") => ScanStep::Skip(2),
        b'?' if bytes[i..].starts_with(b"?->") => ScanStep::Skip(3),
        b'-' | b'?' => ScanStep::Stop,
        b'.' | b'+' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'&' | b'|' | b'^'
        | b',' | b' ' | b'\t' | b'\n' => ScanStep::Stop,
        _ => ScanStep::Skip(1),
    })
    .is_none()
}

/// Resolve a literal expression to its PHP type.
///
/// Returns `Some(PhpType)` for string literals (`"…"`, `'…'`), integer
/// literals (`42`, `-1`), float literals (`3.14`), boolean literals
/// (`true`, `false`), `null`, and array literals (`[…]`).
pub(super) fn resolve_literal_type(text: &str) -> Option<PhpType> {
    // Closure / arrow function literals: fn(...), function(...), and the
    // `static`-prefixed forms of both.
    if crate::completion::source::helpers::is_closure_like_text(text) {
        return Some(PhpType::named(atom("Closure")));
    }

    // String literals: "…" or '…'
    if (text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''))
    {
        return Some(PhpType::named(atom("string")));
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
        return Some(PhpType::named(atom("array")));
    }

    // Numeric literals — try int first, then float.
    // Strip an optional leading minus for negative literals.
    let numeric = text.strip_prefix('-').unwrap_or(text);
    if !numeric.is_empty()
        && numeric.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        && numeric.bytes().any(|b| b.is_ascii_digit())
    {
        return Some(PhpType::named(atom("int")));
    }
    if !numeric.is_empty()
        && numeric
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'_')
        && numeric.bytes().filter(|&b| b == b'.').count() == 1
        && numeric.bytes().any(|b| b.is_ascii_digit())
    {
        return Some(PhpType::named(atom("float")));
    }

    None
}

#[cfg(test)]
mod cast_tests {
    use super::resolve_cast_type;

    fn cast(text: &str) -> Option<String> {
        resolve_cast_type(text).map(|ty| ty.to_string())
    }

    #[test]
    fn a_cast_names_its_result_type() {
        assert_eq!(cast("(string) $value").as_deref(), Some("string"));
        assert_eq!(cast("(int)$value").as_deref(), Some("int"));
        assert_eq!(cast("(bool) $flag").as_deref(), Some("bool"));
        assert_eq!(cast("(float) $n").as_deref(), Some("float"));
        assert_eq!(cast("(array) $thing").as_deref(), Some("array"));
        assert_eq!(cast("(object) $thing").as_deref(), Some("object"));
    }

    #[test]
    fn the_aliases_php_accepts_read_the_same() {
        assert_eq!(cast("(integer) $n").as_deref(), Some("int"));
        assert_eq!(cast("(boolean) $b").as_deref(), Some("bool"));
        assert_eq!(cast("(double) $n").as_deref(), Some("float"));
        assert_eq!(cast("(binary) $s").as_deref(), Some("string"));
    }

    #[test]
    fn a_chain_or_index_operand_still_counts_as_one() {
        assert_eq!(
            cast("(string) $order->customer->id").as_deref(),
            Some("string")
        );
        assert_eq!(cast("(string) $row['name']").as_deref(), Some("string"));
        assert_eq!(cast("(string) $order?->total()").as_deref(), Some("string"));
        assert_eq!(cast("(string) $row['a b']").as_deref(), Some("string"));
    }

    #[test]
    fn a_cast_inside_a_larger_expression_is_left_alone() {
        assert_eq!(cast("(int) $a / 2"), None);
        assert_eq!(cast("(int) $a === $b"), None);
        assert_eq!(cast("(string) $a . $b"), None);
        assert_eq!(cast("(int) $a - 1"), None);
        assert_eq!(cast("(bool) $a && $b"), None);
    }

    #[test]
    fn text_that_is_not_a_cast_answers_nothing() {
        assert_eq!(cast("$value"), None);
        assert_eq!(cast("($value)"), None);
        assert_eq!(cast("(string)"), None);
        assert_eq!(cast("(new Order())->total()"), None);
        assert_eq!(cast("strlen($value)"), None);
    }
}

#[cfg(test)]
mod auth_guard_tests {
    use super::{
        auth_guard_name, first_string_literal_arg, replace_support_carbon_return,
        resolve_validated_shape_at_call,
    };
    use crate::Backend;
    use crate::atom::atom;
    use crate::php_type::PhpType;
    use crate::test_fixtures::{make_class, make_method};
    use crate::type_engine::resolver::ResolutionCtx;
    use crate::type_engine::subject_expr::SubjectExpr;
    use crate::types::ResolvedType;
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
    fn named_validate_rules_are_normalized_before_shape_resolution() {
        let mut request = make_class("Request");
        request.file_namespace = Some(atom("Illuminate\\Http"));
        let request = Arc::new(request);
        let classes = vec![Arc::clone(&request)];
        let class_loader = |name: &str| {
            (name.trim_start_matches('\\') == "Illuminate\\Http\\Request")
                .then(|| Arc::clone(&request))
        };
        assert!(class_loader("\\Illuminate\\Http\\Request").is_some());
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
        let owners = vec![ResolvedType::from_arc(Arc::clone(&request))];

        let shape = resolve_validated_shape_at_call(
            &SubjectExpr::parse("$request"),
            "validate",
            "rules: ['title' => 'required|string']",
            &owners,
            &ctx,
        )
        .expect("the named rules argument should produce a validated shape");

        assert_eq!(shape.to_string(), "array{title: string}");
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
