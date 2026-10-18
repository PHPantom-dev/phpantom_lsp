//! Call return-type resolution: the primary entry point that resolves a
//! structured call expression + argument text to zero or more `ClassInfo`
//! values.  The per-callee arms live in [`call_arms`], the typing of
//! `ClassName::MEMBER` reads in [`class_constants`], literal and operator
//! text in [`literals`] and [`operands`], and the Laravel refinements in
//! [`laravel`].

mod call_arms;
mod class_constants;
mod laravel;
mod literals;
mod operands;

use crate::atom::atom;
use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::find_class_by_name;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::subject_expr::SubjectExpr;
use crate::type_engine::variable::array_func_rules::{
    array_func_element_type, array_func_raw_type,
};
use crate::types::*;

use crate::text_scan::split_top_level_commas;
use crate::type_engine::conditional_resolution::{
    TemplateContext, ThisContext, VarClassStringResolver, receiver_type_for_condition,
    resolve_conditional_with_text_args_and_defaults, resolve_conditional_without_args,
    resolve_conditional_without_args_and_defaults,
};
use crate::type_engine::resolver::ResolutionCtx;

use super::arg_type_resolution::TextArrayFuncArgs;
use super::target_cache::try_infer_body_return_type;

pub(crate) use class_constants::{
    class_constant_type, enum_case_property_literal, folded_global_constant_type,
    resolve_static_access_type,
};
use laravel::{
    replace_support_carbon_return, resolve_auth_user_at_call, resolve_request_accessor_at_call,
    resolve_validated_shape_at_call,
};
pub(crate) use literals::literal_arg_type;
pub(super) use literals::{resolve_literal_type, resolve_operator_type};
pub(super) use operands::resolve_cast_type;

/// Bundled parameters for [`Backend::resolve_method_return_types_with_args`].
///
/// Groups the resolution-context fields that are threaded through method
/// return-type resolution so the function stays within clippy's argument
/// limit.
pub(crate) struct MethodReturnCtx<'a> {
    /// All classes known in the current file.
    pub all_classes: &'a [Arc<ClassInfo>],
    /// Cross-file class resolution callback for already-resolved names.
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
    /// Resolves a class a call-site argument names as written; see
    /// [`ConditionalClassContext::written_class`](crate::type_engine::conditional_resolution::ConditionalClassContext::written_class).
    pub written_class: Option<&'a dyn Fn(&str) -> String>,
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
        written_class: &'a dyn Fn(&str) -> String,
        is_static: bool,
    ) -> Self {
        Self {
            all_classes: ctx.all_classes,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            template_subs,
            var_resolver: Some(var_resolver),
            cache: ctx.resolved_class_cache,
            calling_class_name: ctx.current_class.map(|c| c.fqn().as_str()),
            written_class: Some(written_class),
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
        let placeholder;
        let current_class = match ctx.current_class {
            Some(cc) => cc,
            None => {
                placeholder =
                    crate::class_lookup::class_context_placeholder(ctx.content, ctx.cursor_offset);
                &placeholder
            }
        };
        let var_ctx = crate::type_engine::resolver::VarResolutionCtx {
            source_imports: ctx.source_imports,
            backend: ctx.backend,
            scope_var_resolver: ctx.scope_var_resolver,
            ..crate::type_engine::resolver::VarResolutionCtx::new(
                var_name,
                current_class,
                ctx.all_classes,
                ctx.content,
                ctx.cursor_offset,
                ctx.class_loader,
                ctx.source_class_loader,
            )
        };
        crate::type_engine::variable::class_string_resolution::resolve_class_string_targets(
            var_name,
            ctx.cursor_offset,
            &var_ctx,
        )
        .iter()
        .map(|c| c.fqn().to_string())
        .collect()
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
    call_site: crate::type_engine::conditional_resolution::ConditionalClassContext<'_>,
    owner: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let cond = method.conditional_return()?;
    let bound_cond = crate::type_engine::conditional_resolution::bind_callable_condition_templates(
        cond,
        template_subs,
    );
    let cond = bound_cond.as_ref().unwrap_or(cond);
    let declaring_fqn = owner.fqn();
    let class_values =
        crate::inheritance::class_scoped_template_values(template_subs, method.template_params());
    let this_type = receiver_type_for_condition(
        declaring_fqn.as_str(),
        &owner.template_params,
        template_subs,
    );
    let tpl = TemplateContext {
        defaults: Some(class_values.as_ref()),
        params: method.template_params(),
        bindings: method.template_bindings(),
        arg_type_resolver: None,
        this_type: Some(&this_type),
    };
    let resolved = if !text_args.is_empty() {
        resolve_conditional_with_text_args_and_defaults(
            cond,
            &method.parameters,
            text_args,
            var_resolver,
            crate::type_engine::conditional_resolution::ConditionalClassContext {
                declaring: Some(declaring_fqn.as_str()),
                ..call_site
            },
            class_loader,
            &tpl,
        )
    } else {
        resolve_conditional_without_args_and_defaults(
            cond,
            &method.parameters,
            tpl.defaults,
            Some(ThisContext {
                this_type: &this_type,
                declaring_class_name: declaring_fqn.as_str(),
                class_loader,
            }),
        )
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

    /// The return type hint
    /// [`Self::resolve_call_return_types_expr_with_hint`] captures for a
    /// call, for a caller that wants the type itself (an array to index
    /// into, a scalar) rather than the classes it names.
    pub(crate) fn resolve_call_return_type_hint(
        callee: &SubjectExpr,
        text_args: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let mut hint = None;
        Self::resolve_call_return_types_on_receiver(callee, text_args, None, ctx, Some(&mut hint));
        hint
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
            crate::type_engine::type_resolution::type_hint_to_classes_typed_returned(
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
        return_type_hint_out: Option<&mut Option<PhpType>>,
    ) -> Vec<Arc<ClassInfo>> {
        match callee {
            SubjectExpr::MethodCall { base, method } => {
                let method = crate::type_engine::resolver::resolve_dynamic_member_name(method, ctx);
                Self::return_types_of_method_call(
                    base,
                    &method,
                    text_args,
                    receiver,
                    ctx,
                    return_type_hint_out,
                )
            }
            SubjectExpr::StaticMethodCall { class, method } => {
                let method = crate::type_engine::resolver::resolve_dynamic_member_name(method, ctx);
                Self::return_types_of_static_method_call(
                    class,
                    &method,
                    text_args,
                    ctx,
                    return_type_hint_out,
                )
            }
            SubjectExpr::FunctionCall(func_name) => {
                Self::return_types_of_function_call(func_name, text_args, ctx, return_type_hint_out)
            }
            SubjectExpr::Variable(var_name) => {
                Self::return_types_of_variable_invocation(var_name, ctx, return_type_hint_out)
            }
            SubjectExpr::NewExpr { class_name } => Self::return_types_of_constructor_call(
                class_name,
                text_args,
                ctx,
                return_type_hint_out,
            ),
            _ => Self::return_types_of_other_callee(callee, ctx, return_type_hint_out),
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
        // A conditional nested in the return type (`groupBy()`'s
        // `static<($groupBy is array|string ? array-key : …), …>`) is decided
        // by this call's arguments before the classes are resolved from it,
        // or the type arguments the classes are built with stay raw.
        let decide_nested = |method: &MethodInfo, ty: PhpType| -> PhpType {
            if !ty.contains_conditional() {
                return ty;
            }
            let tpl = TemplateContext {
                defaults: Some(template_subs),
                params: method.template_params(),
                bindings: method.template_bindings(),
                arg_type_resolver: None,
                this_type: None,
            };
            crate::type_engine::conditional_resolution::evaluate_nested_conditionals_text(
                &ty,
                &method.parameters,
                text_args,
                var_resolver,
                crate::type_engine::conditional_resolution::ConditionalClassContext {
                    calling: mr_ctx.calling_class_name,
                    declaring: Some(class_info.fqn().as_str()),
                    written_class: mr_ctx.written_class,
                },
                class_loader,
                &tpl,
            )
        };
        // Helper: try to resolve a method's conditional return type, falling
        // back to template-substituted return type, then plain return type.
        let resolve_method = |method: &MethodInfo| -> Vec<Arc<ClassInfo>> {
            // Try conditional return type first (PHPStan syntax)
            if let Some(effective) = resolve_conditional_return_hint(
                method,
                text_args,
                var_resolver,
                template_subs,
                crate::type_engine::conditional_resolution::ConditionalClassContext {
                    calling: mr_ctx.calling_class_name,
                    declaring: None,
                    written_class: mr_ctx.written_class,
                },
                class_info,
                class_loader,
            ) {
                let classes: Vec<Arc<ClassInfo>> =
                    crate::type_engine::type_resolution::type_hint_to_classes_typed_returned(
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
                    let substituted = decide_nested(method, substituted);
                    let classes: Vec<Arc<ClassInfo>> =
                        crate::type_engine::type_resolution::type_hint_to_classes_typed_returned(
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
                            crate::type_engine::type_resolution::type_hint_to_classes_typed_returned(
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
                return crate::type_engine::type_resolution::type_hint_to_classes_typed_returned(
                    &decide_nested(method, ret.clone()),
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
                return crate::type_engine::type_resolution::type_hint_to_classes_typed_returned(
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
    if crate::type_engine::resolver::is_narrowable_call(expr) {
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
