/// Callable target resolution: resolving a call expression string to a
/// `ResolvedCallableTarget` with label, parameters, and return type, used
/// by signature help and named-argument completion.
use std::sync::Arc;

use crate::Backend;
use crate::atom::atom;
use crate::class_lookup::{find_class_at_offset, resolve_class_keyword};
use crate::php_type::PhpType;
use crate::subject_expr::SubjectExpr;
use crate::types::*;

use crate::completion::conditional_resolution::TemplateContext;
use crate::completion::resolver::ResolutionCtx;
use crate::text_position::position_to_offset;

use tower_lsp::lsp_types::Position;

use super::target_cache::CALLABLE_TARGET_CACHE;

impl Backend {
    /// Resolve an instance method base expression + method name to a
    /// [`ResolvedCallableTarget`].
    ///
    /// Resolves `base` to owner classes, merges each via
    /// `resolve_class_fully_with_generics`, and returns the first match
    /// for `method_name`.
    fn resolve_instance_method_callable(
        base: &SubjectExpr,
        method_name: &str,
        rctx: &ResolutionCtx<'_>,
        args_text: Option<&str>,
    ) -> Option<ResolvedCallableTarget> {
        let subject_text = base.to_subject_text();
        let resolved_types: Vec<ResolvedType> = if base.is_self_like() {
            rctx.current_class
                .map(|c| ResolvedType::from_class(c.clone()))
                .into_iter()
                .collect()
        } else {
            crate::completion::resolver::resolve_target_classes(
                &subject_text,
                crate::AccessKind::Arrow,
                rctx,
            )
        };

        for rt in &resolved_types {
            let owner = match &rt.class_info {
                Some(ci) => Arc::clone(ci),
                None => continue,
            };

            // Extract generic type arguments from the resolved type
            // string (e.g. `Collection<User>` → `[User]`) so we can
            // substitute class-level template parameters in the
            // method's parameter and return types.
            let generic_args: Vec<PhpType> = match &rt.type_string {
                PhpType::Generic(_, args) => args.clone(),
                _ => {
                    // When the resolved type has no generic annotation
                    // but the class declares template parameters (e.g.
                    // `$errors = new Collection()` without `<string>`),
                    // fill in default type args from declared upper
                    // bounds or `mixed`.  This follows PHPStan's
                    // `resolveToBounds()` semantics and prevents raw
                    // template names like `TValue` from leaking into
                    // method parameter and return types.
                    if !owner.template_params.is_empty() {
                        crate::inheritance::default_type_args(&owner)
                    } else {
                        vec![]
                    }
                }
            };

            // ── Callable target cache check ─────────────────────────
            // When args_text is None (argument_count diagnostics),
            // the callable target depends only on the resolved class
            // and method name, not on the specific chain expression.
            // Cache by "FQN::method_lower" so that `$q->where(...)`,
            // `$query->where(...)`, and `Product::query()->where(...)`
            // all share the result.
            //
            // When args_text is Some (type_error diagnostics with
            // method-level template substitution), the result depends
            // on the call-site arguments and cannot be cached this way.
            let method_lower = method_name.to_ascii_lowercase();
            let generic_arg_strings: Vec<String> =
                generic_args.iter().map(|a| a.to_string()).collect();
            let callable_cache_key = if args_text.is_none() {
                let fqn = owner.fqn();
                let key_str = if generic_arg_strings.is_empty() {
                    format!("{}::{}", fqn, method_lower)
                } else {
                    format!(
                        "{}<{}>::{}",
                        fqn,
                        generic_arg_strings.join(","),
                        method_lower
                    )
                };
                Some(key_str)
            } else {
                None
            };

            if let Some(ref key) = callable_cache_key {
                let cached = CALLABLE_TARGET_CACHE.with(|cell| {
                    let borrow = cell.borrow();
                    borrow.as_ref().and_then(|map| map.get(key).cloned())
                });
                match cached {
                    Some(Some(target)) => return Some(target),
                    Some(None) => continue,
                    None => {}
                }
            }

            // Always use a fully-resolved class so that inherited
            // docblock types (return types, parameter types,
            // descriptions) are visible in signature help.  The
            // candidate from `resolve_target_classes` may not have
            // gone through `resolve_class_fully` (e.g. bare `new X`
            // instantiation without generics).
            //
            // Use the fused resolve+substitute helper so that the
            // result of `apply_generic_args` is cached under
            // `(FQN, generic_args)`.  For Eloquent Builder<Model>
            // chains where the same generic class appears at dozens
            // of call sites, this avoids re-cloning and
            // re-substituting hundreds of virtual members each time.
            let effective = crate::virtual_members::resolve_class_fully_with_generics(
                &owner,
                rctx.class_loader,
                rctx.resolved_class_cache,
                &generic_arg_strings,
                &generic_args,
            );

            if let Some(m) = effective.get_method_ci(&method_lower) {
                let mut result_method = m.clone();

                // Apply method-level template substitutions when
                // call-site argument text is available.
                if let Some(at) = args_text {
                    let split_args = crate::completion::types::conditional::split_text_args(at);
                    let method_subs = Self::build_method_template_subs(
                        &effective,
                        method_name,
                        &split_args,
                        rctx,
                    );
                    if !method_subs.is_empty() {
                        crate::inheritance::apply_substitution_to_method(
                            &mut result_method,
                            &method_subs,
                        );
                    }
                    // Collapse any conditionals nested inside the return type
                    // (e.g. `Collection<($k is array|string ? array-key :
                    // …), …>`) against the call arguments so signature help
                    // and downstream consumers never see a raw conditional.
                    if result_method
                        .return_type
                        .as_ref()
                        .is_some_and(|r| r.contains_conditional())
                    {
                        let ret = result_method.return_type.as_ref().unwrap();
                        let arg_ty_resolver = |t: &str| Self::resolve_arg_text_to_type(t, rctx);
                        let tpl = TemplateContext {
                            defaults: Some(&method_subs),
                            params: &result_method.template_params,
                            arg_type_resolver: Some(&arg_ty_resolver),
                        };
                        let evaluated =
                            crate::completion::types::conditional::evaluate_nested_conditionals_text(
                                ret,
                                &result_method.parameters,
                                at,
                                None,
                                rctx.current_class.map(|c| c.name.as_str()),
                                rctx.class_loader,
                                &tpl,
                            );
                        result_method.return_type = Some(evaluated);
                    }
                }

                let target = ResolvedCallableTarget {
                    parameters: result_method.parameters.clone(),
                    return_type: result_method.return_type.clone(),
                    ..Default::default()
                };

                // Store positive result in the callable target cache.
                if let Some(ref key) = callable_cache_key {
                    CALLABLE_TARGET_CACHE.with(|cell| {
                        let mut borrow = cell.borrow_mut();
                        if let Some(ref mut map) = *borrow {
                            map.insert(key.clone(), Some(target.clone()));
                        }
                    });
                }

                return Some(target);
            }

            // Fall back to __call / __callStatic — the candidate
            // directly may contain model-specific members (e.g.
            // Eloquent scope methods injected onto Builder<Model>)
            // that the FQN-keyed cache does not have.
            if let Some(m) = owner.get_method_ci(method_name) {
                let target = ResolvedCallableTarget {
                    parameters: m.parameters.clone(),
                    return_type: m.return_type.clone(),
                    ..Default::default()
                };

                // Store __call fallback in the callable target cache.
                if let Some(ref key) = callable_cache_key {
                    CALLABLE_TARGET_CACHE.with(|cell| {
                        let mut borrow = cell.borrow_mut();
                        if let Some(ref mut map) = *borrow {
                            map.insert(key.clone(), Some(target.clone()));
                        }
                    });
                }

                return Some(target);
            }

            // Store negative result (method not found) in the cache.
            if let Some(ref key) = callable_cache_key {
                CALLABLE_TARGET_CACHE.with(|cell| {
                    let mut borrow = cell.borrow_mut();
                    if let Some(ref mut map) = *borrow {
                        map.insert(key.clone(), None);
                    }
                });
            }
        }
        None
    }

    /// Resolve a static class reference + method name to a
    /// [`ResolvedCallableTarget`].
    ///
    /// Resolves the class via [`crate::completion::resolver::resolve_static_owner_class`], merges
    /// via `resolve_class_fully`, and looks up `method_name`.
    fn resolve_static_method_callable(
        class: &str,
        method_name: &str,
        rctx: &ResolutionCtx<'_>,
        args_text: Option<&str>,
    ) -> Option<ResolvedCallableTarget> {
        let owner = crate::completion::resolver::resolve_static_owner_class(class, rctx)?;

        // When the class has template params, try to substitute them with
        // concrete types. For `parent::` calls, use the child's @extends
        // generics to get the concrete type arguments. Otherwise fall back
        // to upper bounds / `mixed`.
        let merged = if !owner.template_params.is_empty() {
            let type_args = if class.eq_ignore_ascii_case("parent") {
                // Look up the child's extends_generics for the parent class
                rctx.current_class.and_then(|child| {
                    let parent_short = crate::util::short_name(&owner.name);
                    child
                        .extends_generics
                        .iter()
                        .find(|(name, _)| crate::util::short_name(name) == parent_short)
                        .map(|(_, args)| args.clone())
                })
            } else {
                None
            };
            let args = type_args.unwrap_or_else(|| crate::inheritance::default_type_args(&owner));
            crate::virtual_members::resolve_class_fully_with_type_args(
                &owner,
                rctx.class_loader,
                rctx.resolved_class_cache,
                &args,
            )
        } else {
            crate::virtual_members::resolve_class_fully_maybe_cached(
                &owner,
                rctx.class_loader,
                rctx.resolved_class_cache,
            )
        };

        let m = merged.get_method_ci(method_name)?;

        let mut result_method = m.clone();

        // Apply method-level template substitutions when call-site
        // argument text is available.
        if let Some(at) = args_text {
            let split_args = crate::completion::types::conditional::split_text_args(at);
            let method_subs =
                Self::build_method_template_subs(&merged, method_name, &split_args, rctx);
            if !method_subs.is_empty() {
                crate::inheritance::apply_substitution_to_method(&mut result_method, &method_subs);
            }
            // Collapse conditionals nested inside the return type (e.g.
            // `Str::replace`'s `($subject is string ? string : string[])`
            // wrapped in a generic factory) against the call arguments.
            if result_method
                .return_type
                .as_ref()
                .is_some_and(|r| r.contains_conditional())
            {
                let ret = result_method.return_type.as_ref().unwrap();
                let arg_ty_resolver = |t: &str| Self::resolve_arg_text_to_type(t, rctx);
                let tpl = TemplateContext {
                    defaults: Some(&method_subs),
                    params: &result_method.template_params,
                    arg_type_resolver: Some(&arg_ty_resolver),
                };
                let evaluated =
                    crate::completion::types::conditional::evaluate_nested_conditionals_text(
                        ret,
                        &result_method.parameters,
                        at,
                        None,
                        rctx.current_class.map(|c| c.name.as_str()),
                        rctx.class_loader,
                        &tpl,
                    );
                result_method.return_type = Some(evaluated);
            }
        }

        Some(ResolvedCallableTarget {
            parameters: result_method.parameters.clone(),
            return_type: result_method.return_type.clone(),
            ..Default::default()
        })
    }

    /// Build a [`ResolvedCallableTarget`] from a resolved [`FunctionInfo`].
    fn function_to_callable(func: &FunctionInfo) -> ResolvedCallableTarget {
        ResolvedCallableTarget {
            parameters: func.parameters.clone(),
            return_type: func.return_type.clone(),
            overloads: func.overloads.clone(),
            ..Default::default()
        }
    }

    /// Like [`Self::function_to_callable`] but resolves function-level
    /// `@template` parameters from call-site argument text before
    /// building the callable target.  Without this, functions like
    /// `throw_unless($cond)` would report `expects TValue` instead of
    /// the concrete type.
    fn function_to_callable_with_subs(
        func: &FunctionInfo,
        args_text: Option<&str>,
        rctx: &ResolutionCtx<'_>,
    ) -> ResolvedCallableTarget {
        if let Some(at) = args_text
            && !func.template_params.is_empty()
        {
            let split_args: Vec<String> =
                crate::completion::types::conditional::split_text_args(at)
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
            let subs = crate::completion::variable::rhs_resolution::build_function_template_subs(
                func,
                &split_args,
                rctx,
            );
            if !subs.is_empty() {
                let parameters: Vec<_> = func
                    .parameters
                    .iter()
                    .map(|p| {
                        let mut param = p.clone();
                        if let Some(ref mut hint) = param.type_hint {
                            *hint = hint.substitute(&subs);
                        }
                        param
                    })
                    .collect();
                return ResolvedCallableTarget {
                    parameters,
                    return_type: func.return_type.clone(),
                    ..Default::default()
                };
            }
        }
        Self::function_to_callable(func)
    }

    /// Resolve class name keywords (`self`, `static`, `parent`) to actual
    /// class names in the context of the current class.
    fn resolve_class_name_keyword(class_name: &str, current_class: Option<&ClassInfo>) -> String {
        resolve_class_keyword(class_name, current_class).unwrap_or_else(|| class_name.to_string())
    }

    /// Build a [`ResolvedCallableTarget`] for a constructor call.
    ///
    /// Loads and merges the class, then extracts `__construct` parameters.
    /// When `args_text` is provided, class-level `@template` parameters are
    /// resolved from the call-site argument types and substituted into the
    /// constructor's parameter types.
    ///
    /// For example, given `/** @template T */ class Box { /** @param T $value */ … }`,
    /// calling `new Box(new Gift())` resolves `T` → `Gift` and substitutes it
    /// into the constructor parameters so that type-error diagnostics see
    /// `Gift` instead of the raw `T`.
    fn resolve_constructor_callable(
        class_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: &crate::virtual_members::ResolvedClassCache,
        args_text: Option<&str>,
        rctx: &ResolutionCtx<'_>,
    ) -> Option<ResolvedCallableTarget> {
        let ci = class_loader(class_name)?;
        let merged = crate::virtual_members::resolve_class_fully_cached(&ci, class_loader, cache);
        let ctor = match merged.get_method("__construct") {
            Some(c) => c.clone(),
            // A class with no constructor (and none inherited) accepts any
            // arguments without error: PHP silently ignores them. Mark the
            // target so the argument-count diagnostic skips it, while
            // signature help still shows the empty `()` signature.
            None => {
                return Some(ResolvedCallableTarget {
                    parameters: vec![],
                    return_type: None,
                    accepts_any_args: true,
                    ..Default::default()
                });
            }
        };

        // Apply class-level template substitutions from the call-site
        // argument types when the constructor has template bindings.
        if let Some(at) = args_text
            && !ctor.template_bindings.is_empty()
        {
            let split_args = crate::completion::types::conditional::split_text_args(at);
            let subs = Self::build_method_template_subs(&merged, "__construct", &split_args, rctx);
            if !subs.is_empty() {
                let mut result_ctor = ctor;
                crate::inheritance::apply_substitution_to_method(&mut result_ctor, &subs);
                return Some(ResolvedCallableTarget {
                    parameters: result_ctor.parameters.clone(),
                    return_type: result_ctor.return_type.clone(),
                    ..Default::default()
                });
            }
        }

        Some(ResolvedCallableTarget {
            parameters: ctor.parameters.clone(),
            return_type: ctor.return_type.clone(),
            ..Default::default()
        })
    }

    // ── Main callable target resolution ─────────────────────────────────

    /// Resolve a call expression string to the callable's owner class and
    /// method (or standalone function), returning a
    /// [`ResolvedCallableTarget`] with the label, parameters, and return
    /// type.
    ///
    /// This is the single shared implementation used by both signature
    /// help (`resolve_callable`) and named-argument completion
    /// (`resolve_named_arg_params`).  Each caller projects the fields it
    /// needs from the result.
    ///
    /// The `expr` parameter uses the same format as the symbol map's
    /// `CallSite::call_expression`:
    ///   - `"functionName"` for standalone function calls
    ///   - `"$subject->method"` for instance/null-safe method calls
    ///   - `"ClassName::method"` for static method calls
    ///   - `"new ClassName"` for constructor calls
    pub(crate) fn resolve_callable_target(
        &self,
        expr: &str,
        content: &str,
        position: Position,
        file_ctx: &FileContext,
    ) -> Option<ResolvedCallableTarget> {
        self.resolve_callable_target_with_args(expr, content, position, file_ctx, None)
    }

    /// Like [`resolve_callable_target`](Self::resolve_callable_target)
    /// but accepts optional raw argument text for method-level template
    /// substitution.
    ///
    /// When `call_args_text` is `Some("$user, 42")`, method-level
    /// `@template` parameters are resolved from the call-site argument
    /// types and substituted into the parameter types before returning.
    pub(crate) fn resolve_callable_target_with_args(
        &self,
        expr: &str,
        content: &str,
        position: Position,
        file_ctx: &FileContext,
        call_args_text: Option<&str>,
    ) -> Option<ResolvedCallableTarget> {
        let class_loader = self.class_loader(file_ctx);
        let function_loader_cl = self.function_loader(file_ctx);
        let cursor_offset = position_to_offset(content, position);
        let current_class = find_class_at_offset(&file_ctx.classes, cursor_offset);

        let rctx = ResolutionCtx {
            current_class,
            all_classes: &file_ctx.classes,
            content,
            cursor_offset,
            class_loader: &class_loader,
            laravel_macro_this_resolver: None,
            resolved_class_cache: Some(&self.resolved_class_cache),
            function_loader: Some(&function_loader_cl),
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: false,
        };

        let parsed = SubjectExpr::parse(expr);

        // Unwrap `CallExpr` wrapper so downstream arms match the inner
        // callee directly.  Capture `args_text` from the parsed
        // expression; prefer the caller-supplied `call_args_text` when
        // available (it comes from the source content and is more
        // accurate for method-level template substitution).
        let (effective, args_text_from_parse) = match &parsed {
            SubjectExpr::CallExpr { callee, args_text } => {
                (callee.as_ref(), Some(args_text.as_str()))
            }
            other => (other, None),
        };

        let effective_args_text = call_args_text.or(args_text_from_parse);

        let result = match effective {
            // ── Constructor: `new ClassName` or `new ClassName()` ────
            SubjectExpr::NewExpr { class_name } => {
                let resolved_class_name =
                    Self::resolve_class_name_keyword(class_name, rctx.current_class);
                // For a plain (non-keyword) name, resolve it as a
                // source-level reference so a same-namespace class wins
                // over a global stub of the same short name.
                let resolved_class_name = if resolved_class_name == *class_name {
                    let ns = rctx.current_class.and_then(|c| c.file_namespace.as_deref());
                    crate::util::resolve_source_class_name(class_name, ns, &class_loader)
                } else {
                    resolved_class_name
                };
                Self::resolve_constructor_callable(
                    &resolved_class_name,
                    &class_loader,
                    &self.resolved_class_cache,
                    effective_args_text,
                    &rctx,
                )
            }

            // ── Instance method call: `$subject->method(…)` ─────────
            SubjectExpr::MethodCall { base, method } => {
                Self::resolve_instance_method_callable(base, method, &rctx, effective_args_text)
            }

            // ── Static method call: `Class::method(…)` ──────────────
            SubjectExpr::StaticMethodCall { class, method } => {
                Self::resolve_static_method_callable(class, method, &rctx, effective_args_text)
            }

            // ── Standalone function call: `functionName(…)` ─────────
            SubjectExpr::FunctionCall(name) => {
                let func =
                    self.resolve_function_name(name, &file_ctx.use_map, &file_ctx.namespace)?;
                Some(Self::function_to_callable_with_subs(
                    &func,
                    effective_args_text,
                    &rctx,
                ))
            }

            // ── Variable used as a callable target: `$fn(…)` ────────
            // Check for a first-class callable assignment and recurse.
            SubjectExpr::Variable(var_name) => {
                let callable_target =
                    Self::extract_callable_target_from_variable(var_name, content, cursor_offset)?;
                self.resolve_callable_target_with_args(
                    &callable_target,
                    content,
                    position,
                    file_ctx,
                    call_args_text,
                )
            }

            // ── Bare class name used as a function name ─────────────
            // Named-arg and signature-help contexts pass bare function
            // names like `"foo"` which `SubjectExpr::parse` produces
            // as `ClassName` (since it can't distinguish class names
            // from function names without context).
            SubjectExpr::ClassName(name) => {
                let func =
                    self.resolve_function_name(name, &file_ctx.use_map, &file_ctx.namespace)?;
                Some(Self::function_to_callable_with_subs(
                    &func,
                    effective_args_text,
                    &rctx,
                ))
            }

            // ── PropertyChain used as a callable target ──────────────
            // Named-arg and signature-help contexts pass expressions
            // like `"$this->method"` (without trailing `()`), which
            // `SubjectExpr::parse` produces as `PropertyChain`.  Treat
            // the trailing property as a method name.
            SubjectExpr::PropertyChain { base, property } => {
                Self::resolve_instance_method_callable(base, property, &rctx, effective_args_text)
            }

            // ── StaticAccess used as a callable target ──────────────
            // Same situation: `"ClassName::method"` without `()` parses
            // as `StaticAccess` rather than `StaticMethodCall`.
            SubjectExpr::StaticAccess { class, member } => {
                Self::resolve_static_method_callable(class, member, &rctx, effective_args_text)
            }

            // ── Anything else doesn't resolve to a callable ─────────
            _ => None,
        };

        // ── Call-result invocation ──────────────────────────────────
        // When the original expression was a `CallExpr`, the resolved
        // target describes the inner callee (e.g. `makeCallable`), but
        // the actual call is on the callee's *return value*:
        //
        //   makeCallable('1', '2')('test')
        //   ^^^^^^^^^^^^^^^^^^^^^^^^       ← inner callee resolved above
        //                          ^^^^^^^ ← outer call on the return value
        //
        // If the return type is a typed callable (`callable(string): T`)
        // use its parameter signature.  For bare `callable` without a
        // parameter spec, flag `accepts_any_args` so that argument-count
        // diagnostics are suppressed and inlay hints don't show the
        // wrong parameter names.
        if matches!(&parsed, SubjectExpr::CallExpr { .. })
            && let Some(ref target) = result
            && let Some(ref return_type) = target.return_type
            && let Some(invoked) = callable_type_as_target(return_type)
        {
            return Some(invoked);
        }

        result
    }
}

/// Convert a callable `PhpType` to a `ResolvedCallableTarget`.
///
/// Used when a function/method returns a callable type and that return
/// value is immediately invoked: `makeCallable('1', '2')('test')`.
///
/// - `PhpType::Callable { params, return_type, .. }` (typed callable like
///   `callable(string): string`) -> params are converted to `ParameterInfo`.
/// - `PhpType::Named("callable")` or `PhpType::Named("Closure")` (bare
///   callable without parameter specification) -> returns a target with
///   `accepts_any_args: true` so diagnostics are suppressed.
/// - Other types -> returns `None` (not a callable).
fn callable_type_as_target(return_type: &PhpType) -> Option<ResolvedCallableTarget> {
    match return_type {
        PhpType::Callable {
            params,
            return_type,
            ..
        } => {
            let parameters: Vec<ParameterInfo> = params
                .iter()
                .enumerate()
                .map(|(i, p)| ParameterInfo {
                    name: atom(&format!("$param{}", i + 1)),
                    is_required: !p.optional && !p.variadic,
                    type_hint: Some(p.type_hint.clone()),
                    native_type_hint: None,
                    description: None,
                    default_value: None,
                    is_variadic: p.variadic,
                    is_reference: false,
                    closure_this_type: None,
                })
                .collect();
            Some(ResolvedCallableTarget {
                parameters,
                return_type: return_type.as_deref().cloned(),
                accepts_any_args: false,
                ..Default::default()
            })
        }
        PhpType::Named(name)
            if name.eq_ignore_ascii_case("callable") || name.eq_ignore_ascii_case("Closure") =>
        {
            Some(ResolvedCallableTarget {
                parameters: vec![],
                return_type: None,
                accepts_any_args: true,
                ..Default::default()
            })
        }
        PhpType::Union(members) => {
            for member in members {
                if let Some(target) = callable_type_as_target(member) {
                    return Some(target);
                }
            }
            None
        }
        PhpType::Nullable(inner) => callable_type_as_target(inner),
        _ => None,
    }
}
