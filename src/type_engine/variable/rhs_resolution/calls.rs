/// Function/method/static call return-type resolution: resolves
/// function calls, method calls, and static calls to their return
/// types, including template substitution for `@template` parameters
/// and conditional return type evaluation.
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{atom, bytes_to_str};
use crate::php_type::{CallableParam, PhpType, TypeKind};
use crate::types::{ClassInfo, MethodInfo, ResolvedType};
use crate::virtual_members::laravel::validated_shape;

use crate::type_engine::call_resolution::MethodReturnCtx;
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::type_engine::variable::resolution::build_var_resolver_from_ctx;

use super::{
    extract_closure_or_arrow_return_type, resolve_rhs_expression, resolve_var_types,
    resolved_type_with_lookup,
};

/// A call argument's type as the AST walker resolves it, looked up by the
/// argument's source text.
///
/// The argument resolver behind template binding reads an argument as
/// source *text*, so it can only answer for the expression shapes it has a
/// rule for. An operator it does not split (`$a + $b`) or a construct it
/// cannot parse leaves a `@template` unbound even though the walker
/// resolved that very expression on its way into the call. A caller that
/// reached the call through the AST hands this in so the binding modes can
/// ask the walker rather than grow a second set of expression rules.
pub(crate) type ArgWalkerTypes<'a> = &'a dyn Fn(&str) -> Option<PhpType>;

/// Build a template substitution map for a function-level `@template` call.
///
/// Uses the function's `template_bindings` to match template parameters to
/// their concrete types inferred from the call-site arguments.  Handles:
///   - Direct type: `@param T $bar` + `func(new Baz())` → `T = Baz`
///   - Array type: `@param T[] $items` + `func([new X()])` → `T = X`
///   - Generic wrapper: `@param array<TKey, TValue> $v` + `func($users)` →
///     positional resolution through the wrapper's generic arguments.
///
/// The binding itself is [`bind_template_args`](crate::type_engine::call_resolution::bind_template_args),
/// shared with methods and constructors.
pub(crate) fn build_function_template_subs(
    func_info: &crate::types::FunctionInfo,
    arg_texts: &[String],
    walker_types: Option<ArgWalkerTypes<'_>>,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
    let callee = crate::type_engine::call_resolution::TemplateCallee {
        parameters: &func_info.parameters,
        template_bindings: &func_info.template_bindings,
        template_param_bounds: &func_info.template_param_bounds,
    };
    let mut subs = crate::type_engine::call_resolution::bind_template_args(
        &callee,
        &arg_refs,
        walker_types,
        rctx,
    );

    crate::type_engine::call_resolution::finish_template_subs(
        &mut subs,
        &func_info.template_params,
        &func_info.template_param_bounds,
        &func_info.template_param_defaults,
        func_info.return_type.as_ref(),
        rctx,
    );

    subs
}

/// Fill in any function-level `@template` parameter a resolved type still
/// names, using the bindings the call-site arguments provide.
///
/// A conditional return type is evaluated on its own, before the general
/// substitution path runs, so its winning branch can still carry a bare
/// template name (`tap()` returns `TValue`). The arguments are only resolved
/// when the type actually names one, which keeps the common case free.
pub(crate) fn substitute_function_templates(
    func_info: &crate::types::FunctionInfo,
    ty: PhpType,
    arg_texts: &[String],
    walker_types: Option<ArgWalkerTypes<'_>>,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> PhpType {
    if !ty.references_any_name(&func_info.template_params) {
        return ty;
    }
    let subs = build_function_template_subs(func_info, arg_texts, walker_types, rctx);
    if subs.is_empty() {
        return ty;
    }
    ty.substitute(&subs)
}

/// Resolve a variable argument to its raw type string.
///
/// For `$pens` with `/** @var Pen[] $pens */`, returns `Some("Pen[]")`.
/// For `$users` with `/** @var array<int, User> $users */`, returns
/// `Some("array<int, User>")`.
///
/// Tries docblock annotations first, then falls back to AST-based
/// raw type inference.
pub(crate) fn resolve_arg_variable_raw_type(
    arg_text: &str,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let var_name = arg_text.trim();
    // A property chain is read from its base expression below, whatever the
    // base is spelled as (`$this`, `Labels::MEDICAL`, …); everything after
    // that looks the argument up as a variable, so it has to be one.
    if !var_name.starts_with('$') && !var_name.contains("->") {
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
                crate::type_engine::resolver::resolve_target_classes(
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

    // Past this point every lookup is keyed on a variable name, so a chain
    // whose property could not be read has nothing left to answer it.
    if !var_name.starts_with('$') {
        return None;
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
    if crate::type_engine::variable::forward_walk::is_diagnostic_scope_active()
        && !crate::type_engine::variable::forward_walk::is_building_scopes()
    {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        // An entry the walker holds but has no type for says "tracked,
        // unknown", not "mixed" — the walker seeds a subject key before it
        // can answer it. Joining nothing produces `mixed`, which is an
        // answer, so the caller stops looking and a call argument
        // (`array_keys($this->templates())`) never reaches the call
        // resolver that does know its type. Step 3 below already declines
        // an empty entry for the same reason.
        if let Some(types) = crate::type_engine::variable::forward_walk::lookup_diagnostic_scope(
            &prefixed,
            rctx.cursor_offset,
        ) {
            if types.is_empty() {
                return None;
            }
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
    if crate::type_engine::variable::forward_walk::is_building_scopes() {
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

    let default_class;
    let current_class = match rctx.current_class {
        Some(cc) => cc,
        None => {
            default_class =
                crate::class_lookup::class_context_placeholder(rctx.content, rctx.cursor_offset);
            &default_class
        }
    };
    let resolved = crate::type_engine::variable::resolution::resolve_variable_types(
        var_name,
        current_class,
        rctx.all_classes,
        rctx.content,
        rctx.cursor_offset,
        rctx.class_loader,
        rctx.backend,
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
pub(crate) fn resolve_arg_call_raw_type(
    arg_text: &str,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
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
    let expr = crate::type_engine::subject_expr::SubjectExpr::parse(trimmed);
    let crate::type_engine::subject_expr::SubjectExpr::CallExpr { callee, args_text } = &expr
    else {
        return None;
    };
    let mut hint: Option<PhpType> = None;
    Backend::resolve_call_return_types_expr_with_hint(callee, args_text, rctx, Some(&mut hint));
    hint
}

/// Resolve an argument's raw iterable type for positional generic
/// extraction, regardless of the argument's syntax shape.
///
/// Variables and property chains resolve through
/// [`resolve_arg_variable_raw_type`] (docblock annotations, forward-walk
/// scope, assignment scanning); call expressions resolve through the
/// shared call return-type pipeline via [`resolve_arg_call_raw_type`];
/// an element read out of another array goes through
/// [`resolve_arg_dim_raw_type`].  Anything left over is handed to the
/// general argument resolver.
pub(crate) fn resolve_arg_iterable_raw_type(
    arg_text: &str,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    resolve_arg_variable_raw_type(arg_text, rctx)
        .or_else(|| resolve_arg_call_raw_type(arg_text, rctx))
        .or_else(|| resolve_arg_dim_raw_type(arg_text, rctx))
        .or_else(|| Backend::resolve_arg_text_to_type(arg_text, rctx))
}

/// Resolve `$base['key']` by reading one element out of `$base`'s own raw
/// type.
///
/// The general resolver answers with the *classes* an expression can be, so
/// an element that is itself an array — an `array<string, Config>` read out
/// of an `array<string, array<string, Config>>` — comes back from it empty,
/// and a `@template TKey` bound from `array_keys($delta['old'])` has
/// nothing to bind to.  The container's own raw type does carry the answer,
/// so it is resolved through the same entry point (which shortens the text
/// by one subscript each time, so the recursion terminates) and indexed.
fn resolve_arg_dim_raw_type(
    arg_text: &str,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    let inner = trimmed.strip_suffix(']')?;
    let open = crate::text_scan::find_matching_backward(trimmed, trimmed.len() - 1, b'[', b']')?;
    let base = inner[..open].trim();
    // An array literal (`[1, 2, 3]`) is all subscript and no base.
    if base.is_empty() {
        return None;
    }
    let base_type = resolve_arg_iterable_raw_type(base, rctx)?;
    // A shape answers per key, so a literal subscript is looked up by name
    // before falling back to the one element type a generic array has.
    let dim = inner[open + 1..].trim();
    let literal_key = dim
        .strip_prefix('\'')
        .and_then(|d| d.strip_suffix('\''))
        .or_else(|| dim.strip_prefix('"').and_then(|d| d.strip_suffix('"')))
        .unwrap_or(dim);
    base_type
        .shape_value_type(literal_key)
        .cloned()
        .or_else(|| base_type.extract_value_type(false).cloned())
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

/// Whether evaluating any argument in `argument_list` never completes.
///
/// PHP evaluates call arguments before the call itself, so a `never`
/// argument (an expression already proven unreachable, e.g. a variable
/// narrowed to nothing by a prior guard clause) means the call can never
/// be reached either, regardless of what its callee is declared to
/// return.
pub(super) fn any_argument_is_never(
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> bool {
    argument_list.arguments.iter().any(|arg| {
        resolve_rhs_expression(arg.value(), ctx)
            .iter()
            .any(|rt| rt.type_string.is_never())
    })
}

/// Resolve function, method, and static method calls to their return
/// types.
pub(super) fn resolve_rhs_call<'b>(
    call: &'b Call<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let argument_list = match call {
        Call::Function(func_call) => &func_call.argument_list,
        Call::Method(method_call) => &method_call.argument_list,
        Call::NullSafeMethod(method_call) => &method_call.argument_list,
        Call::StaticMethod(static_call) => &static_call.argument_list,
    };
    if any_argument_is_never(argument_list, ctx) {
        return vec![ResolvedType::from_type_string(PhpType::never())];
    }

    let mut resolved = match call {
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
    };

    finish_return_constant_operands(&mut resolved, ctx);
    resolved
}

/// Evaluate the type operators a call's return type reads through a
/// constant.
///
/// A `@return value-of<ID_TABLE>` arrives with the operator still standing:
/// the docblock parser saw a name it could not read, and only the template
/// path reads the constant behind it.  Finish it so the caller gets the
/// value union the table describes rather than a type expression that
/// widens to `mixed`.
pub(super) fn finish_return_constant_operands(
    resolved: &mut [ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) {
    if !resolved
        .iter()
        .any(|rt| rt.type_string.contains_unevaluated_operator())
    {
        return;
    }
    let rctx = ctx.as_resolution_ctx();
    for rt in resolved {
        if let Some(evaluated) =
            crate::type_engine::call_resolution::evaluate_constant_operands(&rt.type_string, &rctx)
        {
            rt.type_string = evaluated;
        }
    }
}

pub(crate) fn infer_closure_literal_type(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    // `$fn(...)` wraps a callable value rather than naming a function, so
    // the closure it makes has that value's own signature.
    if let Expression::PartialApplication(PartialApplication::Function(fpa)) = expr
        && !matches!(fpa.function, Expression::Identifier(_))
    {
        let resolved = resolve_rhs_expression(fpa.function, ctx);
        if let TypeKind::Callable(callable) = ResolvedType::types_joined(&resolved).kind() {
            return PhpType::callable_type(crate::php_type::CallableType {
                kind: atom("Closure"),
                ..(**callable).clone()
            });
        }
    }

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

    // What the body actually produces, independent of any declared
    // return type: an arrow function's expression, or a full closure
    // body's first top-level `return`.
    let body_return = || -> Option<PhpType> {
        let (parameter_list, ret_expr) = match expr {
            Expression::ArrowFunction(arrow) => (&arrow.parameter_list, Some(arrow.expression)),
            Expression::Closure(closure) => (
                &closure.parameter_list,
                closure.body.statements.iter().find_map(|stmt| match stmt {
                    Statement::Return(ret) => ret.value,
                    _ => None,
                }),
            ),
            _ => return None,
        };
        let ret_expr = ret_expr?;

        // The body may read the closure's own parameters (`fn (?string
        // $value): string => $value ?? '-'`), which nothing outside the
        // closure ever assigns, so the outer scope's variable resolution
        // cannot see them. Seed each hinted one so the body resolves
        // against its own signature the way a call to it would.
        let param_types: HashMap<String, PhpType> = parameter_list
            .parameters
            .iter()
            .filter_map(|param| {
                let hint = param.hint.as_ref()?;
                let name = bytes_to_str(param.variable.name).to_string();
                let ty = crate::util::resolve_source_php_type_names(
                    &crate::parser::extract_hint_type(hint),
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.all_classes,
                    ctx.class_loader,
                );
                Some((name, ty))
            })
            .collect();

        let resolved = if param_types.is_empty() {
            resolve_rhs_expression(ret_expr, ctx)
        } else {
            let param_aware_resolver = |name: &str| -> Vec<ResolvedType> {
                match param_types.get(name) {
                    Some(ty) => vec![ResolvedType::from_type_string(ty.clone())],
                    None => resolve_var_types(name, ctx, ctx.cursor_offset),
                }
            };
            let mut param_ctx = ctx.clone();
            param_ctx.scope_var_resolver = Some(&param_aware_resolver);
            resolve_rhs_expression(ret_expr, &param_ctx)
        };

        if resolved.is_empty() {
            None
        } else {
            Some(ResolvedType::types_joined(&resolved))
        }
    };

    let inferred_return = match explicit_or_yield {
        // A closure really returns what its body produces narrowed by
        // what it declares: an explicit `: ReturnType` only wins when
        // the body does not resolve to something narrower (e.g. a bare
        // `: Closure` on a closure whose body returns a closure with a
        // known signature).
        Some(declared) => {
            let declared = crate::util::resolve_source_php_type_names(
                &declared,
                ctx.current_class.file_namespace.as_deref(),
                ctx.all_classes,
                ctx.class_loader,
            );
            let narrowed = body_return().filter(|body| {
                crate::class_lookup::is_subtype_of_typed(body, &declared, ctx.class_loader)
            });
            Some(narrowed.unwrap_or(declared))
        }
        None => body_return().or_else(|| match expr {
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
        }),
    };

    // `static` in a closure's declared return type binds to the class the
    // closure is lexically declared in, the same way it does for a method.
    // A closure has no receiver at the call site to bind it against later,
    // so it must be bound once here, at the point the closure value is
    // created.
    let inferred_return = inferred_return.map(|t| {
        if ctx.current_class.name.is_empty() {
            t
        } else {
            t.resolve_self_refs_bounded(
                &ctx.current_class.fqn(),
                ctx.current_class.parent_class.as_deref(),
            )
        }
    });

    let params = declared_closure_params(expr, ctx);
    if inferred_return.is_some() || !params.is_empty() {
        PhpType::callable_spec("Closure", params, inferred_return)
    } else {
        PhpType::closure()
    }
}

/// The parameter list a closure or arrow function literal declares, as
/// callable-signature parameters.
///
/// A literal's arity and parameter types are part of the type it produces:
/// `fn (BrandView $b) => …` is a `Closure(BrandView): …`, and dropping the
/// parameters makes it fail every declared `Closure(BrandView): …` it is
/// handed to. A parameter with no native hint contributes `mixed`, which a
/// contravariant check accepts from any expected parameter type; a hinted
/// one is qualified against the file's namespace before the global one,
/// the way PHP reads the hint, so `Error` inside `namespace App` names
/// `App\Error` and matches the fully-qualified name in the expectation.
fn declared_closure_params(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<CallableParam> {
    let parameter_list = match expr {
        Expression::Closure(closure) => &closure.parameter_list,
        Expression::ArrowFunction(arrow) => &arrow.parameter_list,
        _ => return Vec::new(),
    };

    parameter_list
        .parameters
        .iter()
        .map(|param| CallableParam {
            type_hint: param
                .hint
                .as_ref()
                .map(|hint| {
                    crate::util::resolve_source_php_type_names(
                        &crate::parser::extract_hint_type(hint),
                        ctx.current_class.file_namespace.as_deref(),
                        ctx.all_classes,
                        ctx.class_loader,
                    )
                })
                .unwrap_or_else(PhpType::mixed),
            optional: param.default_value.is_some(),
            variadic: param.ellipsis.is_some(),
        })
        .collect()
}

/// Answer a call argument's type from the AST expression it was written
/// as, matched by that expression's own source text.
///
/// The text a binding mode is handed came out of one of these spans, so
/// comparing the two finds the expression it names. Two arguments written
/// identically resolve identically, which is what makes matching on the
/// text rather than the position safe here.
pub(crate) fn walker_arg_types<'b, 'c>(
    argument_list: &'b ArgumentList<'b>,
    ctx: &'c VarResolutionCtx<'c>,
) -> impl Fn(&str) -> Option<PhpType> + use<'b, 'c> {
    move |arg_text| {
        let wanted = arg_text.trim();
        argument_list.arguments.iter().find_map(|arg| {
            let value = match arg {
                Argument::Positional(positional) => positional.value,
                Argument::Named(named) => named.value,
            };
            let span = value.span();
            let written = ctx
                .content
                .get(span.start.offset as usize..span.end.offset as usize)?;
            if written.trim() != wanted {
                return None;
            }
            crate::type_engine::variable::resolution::resolve_arg_raw_type(value, ctx)
        })
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
                            crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
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
        // interface), but it is what the Laravel PHPStan extensions infer
        // too.  The Laravel/Carbon ecosystem is written against that model, so
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

        // ── view('name') → concrete Illuminate\View\View ──
        // The helper's conditional return type names the *contract*, but
        // the factory always builds the concrete view object.  Mirrors the
        // `view()` stub the Laravel PHPStan extensions ship.
        if normalized_func.trim_start_matches('\\') == "view" {
            let arg_texts =
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &func_call.argument_list,
                    content,
                );
            if crate::virtual_members::laravel::view_helper_returns_view(
                normalized_func,
                arg_texts.first().map(String::as_str).unwrap_or(""),
            ) && let Some(cls) = (ctx.class_loader)(crate::virtual_members::laravel::VIEW_FQN)
            {
                return ResolvedType::from_classes(vec![cls]);
            }
        }
    }

    // ── Laravel config() return type inference ───────
    if let Some(ref name) = func_name {
        let normalized_func = name.trim_start_matches('\\');
        if matches!(normalized_func, "config" | "Illuminate\\Support\\config") {
            let arg_texts =
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &func_call.argument_list,
                    content,
                );
            if let Some(first_arg) = arg_texts.first()
                && let Some(key) = crate::util::unescape_php_string_literal(first_arg.trim())
                && !key.is_empty()
                && !key.contains('$')
                && let Some(resolver) = ctx.loaders.config_resolver
                && let Some(ty) = resolver(&key)
            {
                return vec![ResolvedType::from_type_string(ty)];
            }
        }
    }

    // ── Laravel translation helper return type narrowing ─────
    // `trans()`/`__()` declare a return type of `string|array|null` because
    // a translation key may name a whole group, and the keyless form hands
    // the key straight back. The key decides which branch a call takes:
    // a leaf entry is a `string`, a group is the array beneath it, and
    // `__()` with no key at all is the `null`.
    if let Some(ref name) = func_name
        && let Some(resolver) = ctx.loaders.trans_resolver
    {
        let normalized_func = name.trim_start_matches('\\');
        if matches!(normalized_func, "trans" | "__") {
            let arg_texts =
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &func_call.argument_list,
                    content,
                );
            match arg_texts.first() {
                Some(first_arg) => {
                    let ty = crate::util::unescape_php_string_literal(first_arg.trim())
                        .filter(|key| !key.is_empty() && !key.contains('$'))
                        .and_then(|key| resolver(&key))
                        .unwrap_or_else(crate::virtual_members::laravel::unresolved_trans_type);
                    return vec![ResolvedType::from_type_string(ty)];
                }
                // `trans()` with no key hands back the translator itself, a
                // shape this narrowing does not model; `__()` returns the
                // null it was given.
                None if normalized_func == "__" => {
                    return vec![ResolvedType::from_type_string(PhpType::null())];
                }
                None => {}
            }
        }
    }

    // ── String builtins over literal arguments ───────
    // The stub declares the widest string the function can return; with
    // literal arguments the call has one answer, and a caller checking
    // it against a literal union needs that answer rather than `string`.
    if let Some(ref name) = func_name
        && let Some(folded) =
            crate::type_engine::variable::raw_type_inference::resolve_string_func_literal_type(
                name,
                &func_call.argument_list,
                ctx,
            )
    {
        return vec![resolved_type_with_lookup(
            folded,
            current_class_name,
            all_classes,
            class_loader,
        )];
    }

    // ── Known array functions ────────────────────────
    // For element-extracting functions (array_pop, etc.)
    // resolve to the element ClassInfo directly.
    if let Some(ref name) = func_name
        && let Some(element_type) =
            crate::type_engine::variable::raw_type_inference::resolve_array_func_element_type(
                name,
                &func_call.argument_list,
                ctx,
            )
    {
        let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &element_type,
            current_class_name,
            all_classes,
            class_loader,
        );
        if !resolved.is_empty() {
            return ResolvedType::from_classes_with_hint(resolved, element_type);
        }
        // The element type is not class-like (`array_pop` on a
        // `list<list<int>>` yields `list<int>`), but it is still the right
        // answer. Returning it as a type-string-only result keeps the one
        // level of unwrapping; falling through to the raw-type branch below
        // would hand back the container type unchanged.
        return vec![resolved_type_with_lookup(
            element_type,
            current_class_name,
            all_classes,
            class_loader,
        )];
    }

    // For type-preserving functions (array_filter, array_values, etc.)
    // the output has the same iterable type as the input array.
    // Return the full type string (e.g. `list<User>`) so that
    // downstream consumers (foreach, array access, hover) see the
    // element type without needing the raw-type pipeline's fallback.
    if let Some(ref name) = func_name
        && let Some(raw_type) =
            crate::type_engine::variable::raw_type_inference::resolve_array_func_raw_type(
                name,
                &func_call.argument_list,
                ctx,
            )
    {
        let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
        // A return type the call site decides (a conditional keyed on an
        // argument, or a branch the flags argument rules out) is
        // authoritative, so it is tried before the declared type.
        if func_info.conditional_return.is_some()
            || crate::type_engine::types::flag_returns::has_flag_dependent_return(name)
        {
            let var_resolver = build_var_resolver_from_ctx(ctx);
            let rctx = ctx.as_resolution_ctx();
            // `is <Type>` conditions on an argument that isn't a literal
            // (`preg_replace($p, $r, $subject)`) are decided by the
            // argument's resolved type, so the branch a call takes matches
            // what it was actually handed.
            let arg_ty_resolver = |t: &str| Backend::resolve_arg_text_to_type(t, &rctx);
            let text_args =
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &func_call.argument_list,
                    content,
                )
                .join(", ");
            let resolved_type = func_info.conditional_return.as_ref().and_then(|cond| {
                let tpl = crate::type_engine::types::conditional::TemplateContext {
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
                    },
                    class_loader,
                    &tpl,
                )
            })
            // A branch the flags argument rules out (`json_encode(…,
            // JSON_THROW_ON_ERROR)` never returning `false`) is decided the
            // same way: at the call site, from the declared return type.
            .or_else(|| {
                crate::type_engine::types::flag_returns::flag_narrowed_return_type(
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
                let walker_types = walker_arg_types(&func_call.argument_list, ctx);
                let ty = substitute_function_templates(
                    &func_info,
                    ty,
                    &crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                        &func_call.argument_list,
                        content,
                    ),
                    Some(&walker_types),
                    &rctx,
                );
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    &ty,
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
                    ty,
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
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &func_call.argument_list,
                    content,
                );
            let rctx = ctx.as_resolution_ctx();
            let walker_types = walker_arg_types(&func_call.argument_list, ctx);
            let subs =
                build_function_template_subs(&func_info, &arg_texts, Some(&walker_types), &rctx);
            if !subs.is_empty()
                && let Some(ref ret) = func_info.return_type
            {
                let substituted = ret.substitute(&subs);
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
            let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
            let resolved = callable_return_resolution(ret_type, ctx);
            if !resolved.is_empty() {
                return resolved;
            }
        }

        // 2. Resolve the variable's own type.  Closures, arrow functions,
        //    and first-class callables are all inferred by
        //    `resolve_rhs_expression` as a `TypeKind::Callable` (see
        //    `infer_closure_literal_type`), so `$fn`'s embedded return
        //    type covers `$fn = function(): T {}`, `$fn = fn(): T => …`,
        //    and `$fn = strlen(...)` / `$fn = $obj->method(...)` alike.
        let var_types = resolve_var_types(&var_name, ctx, ctx.cursor_offset);
        if let Some(ret_type) = joined_callable_return(&var_types) {
            let resolved = callable_return_resolution(&ret_type, ctx);
            if !resolved.is_empty() {
                return resolved;
            }
        }
        for rt in &var_types {
            if let Some(ret_type) = rt.type_string.callable_return_type() {
                let resolved = callable_return_resolution(ret_type, ctx);
                if !resolved.is_empty() {
                    return resolved;
                }
            }
        }

        // 3. An array callable, `[$obj, 'method']`, calls the method.
        for rt in &var_types {
            let resolved = array_callable_return(&rt.type_string, ctx);
            if !resolved.is_empty() {
                return resolved;
            }
        }

        // 4. Check for __invoke().  When $f holds an object with an
        //    __invoke() method, $f() should return __invoke()'s return
        //    type.
        let var_classes = ResolvedType::into_arced_classes(var_types);
        for owner in &var_classes {
            if let Some(invoke) = owner.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
            let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
        // A callable-typed value carries its return type in the type
        // string itself (`callable(): Scope`, `Closure(): Item`), which is
        // how a property or a method result annotated that way arrives
        // here.  Read it the same way the `$fn()` path does before falling
        // back to `__invoke()`.
        if let Some(ret_type) = joined_callable_return(&callee_results) {
            let resolved = callable_return_resolution(&ret_type, ctx);
            if !resolved.is_empty() {
                return resolved;
            }
        }
        for rt in &callee_results {
            if let Some(ret_type) = rt.type_string.callable_return_type() {
                let resolved = callable_return_resolution(ret_type, ctx);
                if !resolved.is_empty() {
                    return resolved;
                }
            }
        }
        for rt in &callee_results {
            if let Some(ref owner_cls) = rt.class_info
                && let Some(invoke) = owner_cls.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        ret_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    if !resolved.is_empty() {
        return ResolvedType::from_classes_with_hint(resolved, ret_type.clone());
    }
    if ret_type.is_empty() {
        return vec![];
    }
    vec![resolved_type_with_lookup(
        ret_type.clone(),
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    )]
}

/// What `(closure)->call($obj)` returns: whatever the closure literal does,
/// run with `$this` bound to `$obj`.
fn closure_call_return(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let closure = crate::parser::unwrap_parens(object);
    if let Some(declared) = extract_closure_or_arrow_return_type(closure) {
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
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    for owner in &owners {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            owner,
            ctx.class_loader,
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

/// A method call's receiver, already resolved: the candidate owner classes
/// plus the `ResolvedType` values they came from.
///
/// The full `ResolvedType` is kept alongside the classes so that the
/// receiver's generic type string (e.g. `Builder<Article>`) is available
/// when the method returns `static`/`self`/`$this`.
pub(super) type MethodReceiver = (Vec<Arc<ClassInfo>>, Vec<ResolvedType>);

/// Resolve a method call's object expression to its receiver.
///
/// `$this` resolves to the enclosing class, a bare variable through the
/// variable pipeline (honouring `match(true)` arm narrowing), and anything
/// else — `(new Factory())`, `getService()`, a chain link — by resolving the
/// expression.
pub(super) fn resolve_method_receiver<'b>(
    object: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> MethodReceiver {
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
        return (classes, vec![]);
    }
    if let Expression::Variable(Variable::Direct(dv)) = object {
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
            return (classes, resolved);
        }
        // Fall back to resolve_target_classes when the variable
        // resolution pipeline returns nothing (e.g. for parameters
        // that are resolved through the completion pipeline's subject
        // resolution).
        let classes: Vec<Arc<ClassInfo>> =
            ResolvedType::into_arced_classes(crate::type_engine::resolver::resolve_target_classes(
                &var,
                crate::types::AccessKind::Arrow,
                &ctx.as_resolution_ctx(),
            ));
        return (classes, vec![]);
    }
    let resolved = resolve_rhs_expression(object, ctx);
    let classes = ResolvedType::into_arced_classes(resolved.clone());
    (classes, resolved)
}

/// Resolve a method call (regular or null-safe) from its constituent parts:
/// the object expression (`$this`, a variable, or an arbitrary chained
/// expression), the method selector, and the argument list.
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
    resolve_method_call_on_receiver(object, method, argument_list, None, ctx)
}

/// Resolve a method call whose receiver may already be known.
///
/// A `Some(receiver)` skips resolving `object`, which is how a fluent chain
/// is walked outward from its base without recursing into each link (see
/// `resolve_method_chain` in the parent module).  `object` is still needed
/// for the parts of resolution that read the receiver's *syntax* rather
/// than its type: whether the call forwards late static binding, and which
/// request a Laravel validation shape belongs to.
pub(super) fn resolve_method_call_on_receiver<'b>(
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    receiver: Option<MethodReceiver>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    if any_argument_is_never(argument_list, ctx) {
        return vec![ResolvedType::from_type_string(PhpType::never())];
    }
    resolve_member_call_on_receiver(object, method, argument_list, receiver, false, ctx)
}

/// [`resolve_method_call_on_receiver`] for either call form.
///
/// `is_static` marks `$obj::method()`, which PHP dispatches on the class of
/// the object `$obj` holds: every rule of an instance call applies, except
/// that a missing method falls back to `__callStatic` rather than `__call`.
fn resolve_member_call_on_receiver<'b>(
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    receiver: Option<MethodReceiver>,
    is_static: bool,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let method_name = match method {
        ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
        // Variable method name (`$obj->$method()`) — see
        // `runtime_named_member_type`.
        _ => return super::runtime_named_member_type(),
    };
    if method_name.eq_ignore_ascii_case("call")
        && let Some(returned) = closure_call_return(object, argument_list, ctx)
    {
        return returned;
    }
    let (owner_classes, receiver_resolved) =
        receiver.unwrap_or_else(|| resolve_method_receiver(object, ctx));

    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
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

    // A property read through the Reflection API: the name handed to
    // `getProperty()` decides the type, so the stub's `ReflectionProperty`
    // / `mixed` return types are as specific as an annotation can be.
    // Keyed on the receiver's own type arguments rather than on an owner
    // class, since it is the reflected class the result depends on.
    if crate::type_engine::call_resolution::is_reflected_property_call(&method_name)
        && let Some(ty) = crate::type_engine::call_resolution::resolve_reflected_property_at_call(
            &method_name,
            &arg_refs,
            &receiver_resolved,
            &rctx,
        )
    {
        let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &ty,
            "",
            ctx.all_classes,
            ctx.class_loader,
        );
        return if classes.is_empty() {
            vec![ResolvedType::from_type_string(ty)]
        } else {
            ResolvedType::from_classes_with_hint(classes, ty)
        };
    }

    // Laravel validated input: the rules that guard this request describe the
    // array it hands back, so `$data = $request->validated()` gets a shape
    // rather than plain `array`.  Classifying the call does not depend on the
    // receiver, so it happens once rather than once per owner.
    //
    // `validate([…])` reads its rules from its own argument, so it works
    // wherever it is written; every other form asks the scope for them and is
    // a no-op without the server state that reads them.
    let shape_call = validated_shape::shape_bearing_method(&method_name)
        .filter(|call| *call == validated_shape::ShapeCall::Validate || ctx.backend.is_some());

    // Laravel request input: `header('X', '')`, `query()`, `file('photo')`
    // and the rest declare one union spanning every way of calling them,
    // and the arguments say which of those ways this is.  Classified once
    // for the same reason the shape call above is.
    let input_accessor =
        crate::virtual_members::laravel::request_input::input_accessor(&method_name);

    for owner in &owner_classes {
        if let Some(result) =
            try_resolve_config_method_type(&owner.fqn(), &method_name, argument_list, ctx)
        {
            return result;
        }
        if let Some(result) =
            try_resolve_trans_method_type(&owner.fqn(), &method_name, argument_list, ctx)
        {
            return result;
        }
        if let Some(result) =
            try_resolve_command_accessor_type(owner, &method_name, argument_list, ctx)
        {
            return result;
        }
        if let Some(call) = shape_call
            && let Some(shape) = try_resolve_validated_shape(owner, call, object, &arg_refs, ctx)
        {
            return vec![ResolvedType::from_type_string(shape)];
        }
        if let Some(accessor) = input_accessor
            && let Some(result) = try_resolve_request_accessor_type(
                owner,
                accessor,
                &method_name,
                argument_list,
                &arg_refs,
                ctx,
            )
        {
            return result;
        }
    }

    // Laravel factory count state: `create()`/`make()` build a single
    // model, or a collection of them when the chain set a count
    // (`factory(3)`, `count(3)`, `times(3)`).  Laravel declares both
    // outcomes as one `Collection<int, TModel>|TModel` return type, so
    // without this the union survives into every assignment, argument and
    // property write that a factory chain feeds.
    if let Some((classes, hint)) = crate::virtual_members::laravel::resolve_factory_count_return_ast(
        object,
        &method_name,
        &receiver_resolved,
        ctx.content,
        &rctx,
    ) {
        return vec![match classes.first() {
            Some(class) => ResolvedType::from_both_arc(hint, Arc::clone(class)),
            None => ResolvedType::from_type_string(hint),
        }];
    }

    // A fluent factory call (`state()`, `for()`, `hasPosts()`, `count()`)
    // returns the factory itself, so the count state the chain has built
    // up so far travels on to the result — which is what lets a `create()`
    // several statements and one variable later still know what it builds.
    // The argument type is resolved lazily: only a non-literal argument to
    // Laravel's nullable `count()` needs it to distinguish int, null and a
    // value that could be either.
    let first_arg_type = || {
        let arg = argument_list.arguments.first()?;
        let resolved = resolve_rhs_expression(arg.value(), ctx);
        (!resolved.is_empty()).then(|| ResolvedType::types_joined(&resolved))
    };
    let fluent_count = crate::virtual_members::laravel::fluent_factory_count(
        &receiver_resolved,
        &method_name,
        arg_refs.first().copied(),
        &first_arg_type,
        &rctx,
    );

    let receiver_is_this = matches!(
        object,
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
    );
    let lsb_class = lsb_class_for_call(receiver_is_this, &receiver_resolved, ctx);

    let is_union = owner_classes.len() > 1;
    let mut union_results: Vec<ResolvedType> = Vec::new();
    // A receiver that is every one of its classes at once satisfies each of
    // their declarations, so what they return is intersected, not unioned.
    let receiver_is_intersection = is_union
        && !receiver_resolved.is_empty()
        && receiver_resolved
            .iter()
            .all(|rt| matches!(rt.type_string.kind(), TypeKind::Intersection(_)));
    let mut intersected_results: Vec<Vec<ResolvedType>> = Vec::new();

    for (idx, owner) in owner_classes.iter().enumerate() {
        // Build class-level template substitutions from the receiver's
        // generic type string (e.g. `Collection<int, User>` maps
        // `TKey => int, TValue => User`), merge in method-level
        // substitutions bound from the call's arguments, then override
        // with any `@psalm-if-this-is` inference from the receiver's
        // concrete type.
        let receiver_type = receiver_resolved
            .get(idx)
            .or_else(|| receiver_resolved.first())
            .map(|rt| &rt.type_string);
        let template_subs = crate::type_engine::call_resolution::build_call_template_subs(
            owner,
            &method_name,
            &arg_refs,
            receiver_type,
            &rctx,
        );

        // When the return type contains `static`/`self`/`$this` and the
        // receiver was resolved with generic parameters, use the
        // receiver's full type (e.g. `Builder<Article>`) for
        // substitution so the generics are preserved; otherwise fall
        // back to a plain FQN swap.
        let owner_key = owner.fqn();
        let receiver_intersection = receiver_intersection_for_owner(&receiver_resolved, &owner_key);
        let self_replace =
            |ty: &PhpType| match receiver_type_for_owner(&receiver_resolved, &owner_key) {
                Some(rt) => ty.replace_self_with_type(&rt),
                // An intersection receiver keeps every member: whichever
                // class late static binding lands on satisfies all of them,
                // not only the one that declared the method.
                None => match receiver_intersection {
                    Some(ref inter) => ty.replace_self_over_type(&owner_key, inter),
                    None => ty.replace_self_bound(&owner_key, lsb_class.as_deref()),
                },
            };

        let mut owner_results = resolve_owner_method_call(
            owner,
            &method_name,
            argument_list,
            ctx,
            is_static,
            &template_subs,
            &self_replace,
        );
        if let Some(count) = fluent_count {
            crate::virtual_members::laravel::carry_factory_count(
                &mut owner_results,
                &receiver_resolved,
                count,
            );
        }
        if !is_union {
            return owner_results;
        }
        if receiver_is_intersection {
            if !owner_results.is_empty() {
                intersected_results.push(owner_results);
            }
            continue;
        }
        ResolvedType::extend_unique(&mut union_results, owner_results);
    }
    if receiver_is_intersection {
        return intersect_owner_results(intersected_results);
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

/// Combine what each member of an intersection receiver returns for the same
/// call into one intersection type.
///
/// Each entry of `per_owner` is one member's result.  A member returning
/// `mixed` adds nothing a sibling's concrete type does not already say.
/// When every remaining result is an object type the members are intersected
/// (`Foo&AnotherFoo`), with each resulting entry carrying the whole
/// intersection the way an intersection-typed variable does.  Anything else
/// (a scalar, a union, a nullable) is joined as a union.
fn intersect_owner_results(mut per_owner: Vec<Vec<ResolvedType>>) -> Vec<ResolvedType> {
    let is_mixed = |group: &[ResolvedType]| group.iter().all(|rt| rt.type_string.is_mixed());
    if per_owner.iter().any(|group| !is_mixed(group)) {
        per_owner.retain(|group| !is_mixed(group));
    }
    if per_owner.len() < 2 {
        return per_owner.pop().unwrap_or_default();
    }

    let mut members: Vec<PhpType> = Vec::new();
    let mut all_objects = true;
    for group in &per_owner {
        let joined = ResolvedType::types_joined(group);
        match joined.kind() {
            TypeKind::Intersection(parts) => {
                for part in parts.iter() {
                    if !members.contains(part) {
                        members.push(part.clone());
                    }
                }
            }
            TypeKind::Nullable(_) => {
                all_objects = false;
                break;
            }
            _ if joined.is_object_like() => {
                if !members.contains(&joined) {
                    members.push(joined);
                }
            }
            _ => {
                all_objects = false;
                break;
            }
        }
    }

    let intersection = (all_objects && members.len() > 1).then(|| PhpType::intersection(members));
    let mut results: Vec<ResolvedType> = Vec::new();
    for mut rt in per_owner.into_iter().flatten() {
        if let Some(ref intersection) = intersection {
            rt.type_string = intersection.clone();
        }
        ResolvedType::push_unique(&mut results, rt);
    }
    results
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
    let union_members = match &rt.type_string.kind() {
        TypeKind::Union(members) => members,
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
        .filter(|m| matches!(m.kind(), TypeKind::Generic(g) if is_same_base(&g.name)))
        .collect();
    if generic_branches.len() < 2 {
        return (owner_classes, receiver_resolved);
    }

    // Expand: for each generic branch, apply the type args to produce
    // a substituted ClassInfo.
    let mut expanded_owners: Vec<Arc<ClassInfo>> = Vec::new();
    let mut expanded_resolved: Vec<ResolvedType> = Vec::new();

    for member in union_members {
        match member.kind() {
            TypeKind::Generic(g) if is_same_base(&g.name) => {
                let arc = crate::virtual_members::resolve_class_fully_with_type_args(
                    base_cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                    &g.args,
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
                expanded_resolved.push(ResolvedType::from_type_string(other.clone().into()));
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
        if !matches!(rt.type_string.kind(), TypeKind::Generic(_)) {
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

/// The receiver's intersection type string, when `owner_name` is one of the
/// classes it intersects.
///
/// A receiver typed `IfaceA&IfaceB` resolves to one entry per member, each
/// carrying the whole intersection as its type string (see
/// [`ResolvedType::tag_as_intersection`](crate::types::ResolvedType::tag_as_intersection)).
/// Calling a method `IfaceA` declares walks the members one at a time, and
/// without this the `static` it returns would come back as bare `IfaceA` —
/// dropping a half of the type the caller already proved.
pub(super) fn receiver_intersection_for_owner(
    receiver_resolved: &[ResolvedType],
    owner_name: &str,
) -> Option<PhpType> {
    let owner_short = crate::util::short_name(owner_name);
    receiver_resolved
        .iter()
        .find(|rt| {
            matches!(rt.type_string.kind(), TypeKind::Intersection(_))
                && rt.class_info.as_ref().is_some_and(|ci| {
                    ci.fqn().as_str() == owner_name || ci.name.as_str() == owner_short
                })
        })
        .map(|rt| rt.type_string.clone())
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
    var_resolver: crate::type_engine::conditional_resolution::VarClassStringResolver<'_>,
    calling_class_name: &str,
    owner: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    template_subs: &HashMap<String, PhpType>,
    arg_type_resolver: crate::type_engine::conditional_resolution::ArgTypeResolver<'_>,
    replace_self: impl Fn(&PhpType) -> PhpType,
) -> Option<PhpType> {
    let method = method_ref?;
    let cond = method.conditional_return.as_ref()?;
    let params = method.parameters.as_slice();
    let declaring_fqn = owner.fqn();
    let class_ctx = crate::type_engine::conditional_resolution::ConditionalClassContext {
        calling: Some(calling_class_name),
        declaring: Some(declaring_fqn.as_str()),
    };
    let class_values =
        crate::inheritance::class_scoped_template_values(template_subs, &method.template_params);
    let this_type = crate::type_engine::conditional_resolution::receiver_type_for_condition(
        declaring_fqn.as_str(),
        &owner.template_params,
        template_subs,
    );
    let tpl = crate::type_engine::conditional_resolution::TemplateContext {
        defaults: Some(class_values.as_ref()),
        params: method.template_params.as_slice(),
        bindings: method.template_bindings.as_slice(),
        arg_type_resolver,
        this_type: Some(&this_type),
    };
    let resolved =
        crate::type_engine::conditional_resolution::resolve_conditional_with_text_args_and_defaults(
            cond,
            params,
            text_args,
            var_resolver,
            class_ctx,
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
        let tpl2 = crate::type_engine::conditional_resolution::TemplateContext {
            defaults: Some(class_values.as_ref()),
            params: method.template_params.as_slice(),
            bindings: method.template_bindings.as_slice(),
            arg_type_resolver,
            this_type: Some(&this_type),
        };
        crate::type_engine::conditional_resolution::evaluate_nested_conditionals_text(
            &substituted,
            params,
            text_args,
            var_resolver,
            class_ctx,
            class_loader,
            &tpl2,
        )
    } else {
        substituted
    };
    if collapsed.is_void() {
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
/// read `.type_string` still see it.
pub(super) fn resolve_from_authoritative_type(
    ty: PhpType,
    current_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<ResolvedType> {
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &ty,
        current_class_name,
        all_classes,
        class_loader,
    );
    if !classes.is_empty() {
        return ResolvedType::from_classes_with_hint(classes, ty);
    }
    vec![resolved_type_with_lookup(
        ty,
        current_class_name,
        all_classes,
        class_loader,
    )]
}

/// Whether `method_name` on `class_info` is declared `static`, looking through
/// the inheritance merge so an inherited or `@method static` member counts.
///
/// A method the merge cannot find is treated as static: an unknown target
/// reached through `ClassName::` is a static call as far as anything we can
/// still say about it goes.
fn method_is_static(class_info: &ClassInfo, method_name: &str, ctx: &VarResolutionCtx<'_>) -> bool {
    if let Some(method) = class_info.get_method_ci(method_name) {
        return method.is_static;
    }
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        class_info,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    merged
        .get_method_ci(method_name)
        .is_none_or(|method| method.is_static)
}

/// The class a `@return static` / `@return $this` annotation binds to for this
/// call, or `None` when the call fixes the class and the keyword collapses.
///
/// PHP only carries late static binding across a *forwarding* call: `$this->`,
/// `self::`, `static::`, and `parent::`.  Writing the class out — `A::create()`,
/// `(new A)->create()`, a variable declared `A` — pins it, so `static` there is
/// exactly `A` however `A` is subclassed.  PHPStan and Psalm both collapse
/// those, and keeping a bounded `static(A)` would claim an openness the call
/// does not have.
///
/// A receiver that is *itself* still bounded keeps the chain open, so
/// `$this->self()->self()` stays bound to the class the chain started from
/// rather than collapsing at the second hop.
fn lsb_class_for_call(
    receiver_is_this: bool,
    receiver_resolved: &[ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) -> Option<crate::atom::Atom> {
    if receiver_is_this {
        return Some(ctx.current_class.fqn());
    }
    receiver_resolved
        .iter()
        .find_map(|rt| match rt.type_string.kind() {
            TypeKind::StaticType(bound) | TypeKind::ThisType(bound) => Some(*bound),
            _ => None,
        })
}

/// The types the call site's arguments resolve to, indexed by the
/// parameter each one binds to.
///
/// A parameter no argument supplies, or whose argument resolves to
/// nothing, reads back as `mixed`: uninformative, so the body falls back
/// to what its own signature says about it.
///
/// Resolving an argument costs as much as resolving any other
/// expression, and the great majority of calls never need this, so the
/// result is cached in `cell` for the several fallbacks that may each ask
/// for it while resolving one call.
fn resolve_call_site_arg_types<'c>(
    method: Option<&MethodInfo>,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
    cell: &'c std::cell::OnceCell<Vec<PhpType>>,
) -> &'c [PhpType] {
    cell.get_or_init(|| {
        let Some(method) = method else {
            return Vec::new();
        };
        if argument_list.arguments.is_empty() || method.parameters.is_empty() {
            return Vec::new();
        }
        let types: Vec<PhpType> =
            crate::call_args::bind_args_to_params(&method.parameters, argument_list)
                .into_iter()
                .map(|bound| {
                    let Some(expr) = bound else {
                        return PhpType::mixed();
                    };
                    let resolved = resolve_rhs_expression(expr, ctx);
                    if resolved.is_empty() {
                        PhpType::mixed()
                    } else {
                        ResolvedType::types_joined(&resolved)
                    }
                })
                .collect();
        // A call that decided nothing asks the same question as no call
        // site at all, so answer it under the same memo key rather than
        // walking the body again for every such site.
        if types.iter().any(PhpType::is_informative) {
            types
        } else {
            Vec::new()
        }
    })
}

/// A declared return type replaced by the one the arguments decide, when
/// they decide something strictly more specific.
///
/// `getProperty(\ReflectionClass $r, string $name): \ReflectionProperty`
/// really returns a `ReflectionProperty<Configuration, 'shell'>` once the
/// call site has fixed both arguments, but the declaration erases that
/// and every caller further along the chain loses it. Reading the body
/// recovers it, and the result is only accepted when it is the declared
/// class with type arguments added — a strict subtype, so it can narrow
/// the declaration but never contradict it.
///
/// Reading a body where a return type is already declared is the
/// expensive case, so this only runs while an outer body-return
/// inference is already walking a body: that is the only place a
/// refinement still has a caller to reach.
fn refine_declared_return(
    declared: PhpType,
    method: Option<&MethodInfo>,
    owner: &ClassInfo,
    ctx: &VarResolutionCtx<'_>,
    call_args: &dyn Fn() -> Vec<PhpType>,
) -> PhpType {
    if !crate::type_engine::call_resolution::body_inference_in_progress() {
        return declared;
    }
    let (Some(method), Some(backend)) = (method, ctx.backend) else {
        return declared;
    };
    // Only a bare class name can gain type arguments, and only a real
    // method has a body to read them out of.
    if method.name_offset == 0
        || method.is_virtual
        || method.parameters.is_empty()
        || !matches!(declared.kind(), TypeKind::Named(_))
    {
        return declared;
    }
    let Some(declared_name) = declared.base_name() else {
        return declared;
    };
    // Nothing the call decided means nothing the declaration doesn't
    // already say, so there is no refinement to go looking for.
    let args = call_args();
    if args.is_empty() {
        return declared;
    }
    let Some(inferred) = crate::type_engine::call_resolution::try_infer_body_return_type(
        backend,
        &owner.fqn(),
        method,
        &args,
    ) else {
        return declared;
    };
    let refines = matches!(inferred.kind(), TypeKind::Generic(g) if !g.args.is_empty())
        && inferred
            .base_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(declared_name));
    if refines { inferred } else { declared }
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
    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let text_args = arg_texts.join(", ");
    let rctx = ctx.as_resolution_ctx();
    let var_resolver = build_var_resolver_from_ctx(ctx);

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
    // `@implements Collection<string, User>`).  The effective type is
    // stored name-resolved while the native hint keeps the spelling from
    // the source, so the two are compared as types, not verbatim.
    let echoes_native = |m: &MethodInfo| match (&m.return_type, &m.native_return_type) {
        (Some(ret), Some(native)) => ret.equivalent(native),
        (ret, native) => ret == native,
    };
    let method_ref = match (owner_method, merged_method) {
        (Some(om), Some(mm)) if echoes_native(om) && !echoes_native(mm) => Some(mm),
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
                .map(|p| PhpType::named(atom(p.as_ref())))
                .unwrap_or(substituted)
        } else {
            substituted
        };
        let resolved = if substituted.contains_self_ref() {
            self_replace(&substituted)
        } else {
            substituted
        };
        // Eloquent's own `get()`/`all()`/relation accessors are annotated
        // with the base `Collection<int, TModel>`; once TModel is concrete
        // we know which collection subclass the model really builds.
        crate::virtual_members::laravel::replace_eloquent_collections_in_type(
            &resolved,
            ctx.class_loader,
        )
        .unwrap_or(resolved)
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
        owner,
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
            let tpl = crate::type_engine::conditional_resolution::TemplateContext {
                defaults: Some(template_subs),
                params: method_ref
                    .map(|m| m.template_params.as_slice())
                    .unwrap_or(&[]),
                bindings: method_ref
                    .map(|m| m.template_bindings.as_slice())
                    .unwrap_or(&[]),
                arg_type_resolver: Some(&arg_ty_resolver),
                this_type: None,
            };
            crate::type_engine::conditional_resolution::evaluate_nested_conditionals_text(
                &ty,
                params,
                &text_args,
                Some(&var_resolver),
                crate::type_engine::conditional_resolution::ConditionalClassContext {
                    calling: Some(current_class_name),
                    declaring: Some(owner.fqn().as_str()),
                },
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

    // The argument types, resolved at most once however many of the
    // fallbacks below end up asking for them.
    let resolved_args = std::cell::OnceCell::new();
    let call_args =
        || resolve_call_site_arg_types(method_ref, argument_list, ctx, &resolved_args).to_vec();

    let ret_type_string = native_ret_type_string
        .map(|d| refine_declared_return(d, method_ref, owner, ctx, &call_args));

    let mr_ctx = MethodReturnCtx {
        all_classes: ctx.all_classes,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        template_subs,
        var_resolver: Some(&var_resolver),
        cache: ctx.resolved_class_cache,
        calling_class_name: Some(&ctx.current_class.name),
        is_static,
        call_args: Some(&call_args),
    };

    let results =
        Backend::resolve_method_return_types_with_args(owner, method_name, &text_args, &mr_ctx);
    if !results.is_empty() {
        // A `mixed` hint carries no information. `results` being non-empty
        // despite it means `resolve_method_return_types_with_args` reached
        // its own body-inference fallback and resolved real classes —
        // attaching the stale `mixed` hint on top would make the resolved
        // type display as `mixed` instead of the inferred class.
        let hint = ret_type_string.filter(|t| !t.is_mixed());
        return match hint {
            Some(hint) => ResolvedType::from_classes_with_hint(results, hint),
            None => ResolvedType::from_classes(results),
        };
    }

    // Body return type inference fallback: when the method has no declared
    // return type, or its only declared type is `mixed` (native or
    // docblock — carries no information, so reading the body can only
    // narrow it, never contradict it), try to infer the return type from
    // the method body.  This handles non-class types (list<Foo>, int,
    // array shapes) that resolve_method_return_types_with_args cannot
    // represent.  Tried before the type-string-only fallback below so
    // that a `mixed` hint doesn't win by default when inference has
    // something better to offer.
    if method_ref.is_some_and(|m| {
        m.return_type.as_ref().is_none_or(|t| t.is_mixed()) && m.name_offset != 0 && !m.is_virtual
    }) && let Some(backend) = ctx.backend
        && let Some(inferred) = crate::type_engine::call_resolution::try_infer_body_return_type(
            backend,
            &owner.fqn(),
            method_ref.unwrap(),
            &call_args(),
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

    // The method has a return type string but `type_hint_to_classes_typed`
    // found no matching class (e.g. `list<Widget>`, `int`, `array{name:
    // string}`), or inference above produced nothing useful.  Return a
    // type-string-only entry so that consumers reading `.type_string`
    // (hover, foreach resolution, null-coalesce stripping) still get the
    // information.
    //
    // Return the type string even for non-informative types like `array` or
    // `mixed` — a correct-but-vague type is better than keeping the
    // previous (wrong) type after reassignment.  Also expand type aliases
    // before returning so that `@phpstan-type UserList array<int, User>`
    // with `@return UserList` is expanded to its concrete type.
    if let Some(hint) = ret_type_string {
        let expanded = crate::type_engine::type_resolution::resolve_type_alias_typed(
            &hint,
            &owner.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        let parsed_effective = expanded.unwrap_or(hint);
        return vec![resolved_type_with_lookup(
            parsed_effective,
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
    // `Cls::{$expr}()` / `Cls::$name()` — see `runtime_named_member_type`.
    if !matches!(static_call.method, ClassLikeMemberSelector::Identifier(_)) {
        return super::runtime_named_member_type();
    }

    let current_class_name: &str = &ctx.current_class.name;

    // `self::`, `static::`, and `parent::` all forward late static binding, so
    // `@return static` on the target stays bound to the class the call is made
    // *from* — including `parent::`, which reads the annotation off the parent
    // but still resolves `static` to the current class.
    let forwards_lsb = matches!(
        static_call.class,
        Expression::Self_(_) | Expression::Static(_) | Expression::Parent(_)
    );

    let class_name = match static_call.class {
        Expression::Self_(_)
        | Expression::Static(_)
        | Expression::Parent(_)
        | Expression::Identifier(_) => crate::class_lookup::class_expression_name(
            static_call.class,
            ctx.current_class,
            ctx.all_classes,
            ctx.class_loader,
        ),
        // ── `$var::method()` where `$var` holds a class-string ──
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name).to_string();
            let targets =
                crate::type_engine::variable::class_string_resolution::resolve_class_string_targets(
                    &var_name,
                    ctx.current_class,
                    ctx.all_classes,
                    ctx.content,
                    ctx.cursor_offset,
                    ctx.class_loader,
                    ctx.backend,
                );
            // When there are multiple possible class targets (union class-string),
            // resolve the method return type through each and union the results.
            if targets.len() > 1 {
                if let ClassLikeMemberSelector::Identifier(ident) = &static_call.method {
                    let method_name_str = bytes_to_str(ident.value).to_string();
                    let mut union_types: Vec<PhpType> = Vec::new();
                    let mut union_classes: Vec<ResolvedType> = Vec::new();
                    for target in &targets {
                        let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
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
                            backend: ctx.backend,
                            template_subs: &template_subs,
                            var_resolver: Some(&var_resolver),
                            cache: ctx.resolved_class_cache,
                            calling_class_name: Some(&ctx.current_class.name),
                            is_static: true,
                            // Only reached when the target has no such
                            // method, so the call resolves through a
                            // magic method with no parameters to seed.
                            call_args: None,
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
                                // Each target is a concrete class-string, so
                                // late static binding has nothing left to
                                // resolve on this branch.
                                let resolved = substituted.replace_self_bound(&target.fqn(), None);
                                union_types.push(resolved);
                            }
                        } else {
                            let results = Backend::resolve_method_return_types_with_args(
                                target,
                                &method_name_str,
                                &text_args,
                                &mr_ctx,
                            );
                            for r in results {
                                union_classes.push(ResolvedType::from_both_arc(
                                    PhpType::named(atom(r.name.as_ref())),
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
                            PhpType::union(union_types)
                        };
                        let resolved_classes =
                            crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
                // None of the targets yielded a return type.
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
                let class_string = resolved.iter().find_map(|rt| match &rt.type_string.kind() {
                    TypeKind::ClassString(Some(inner)) => inner.base_name().map(|s| s.to_string()),
                    TypeKind::Nullable(inner) => match inner.kind() {
                        TypeKind::ClassString(Some(cs_inner)) => {
                            cs_inner.base_name().map(|s| s.to_string())
                        }
                        _ => None,
                    },
                    TypeKind::Union(members) => members.iter().find_map(|m| match m.kind() {
                        TypeKind::ClassString(Some(inner)) => {
                            inner.base_name().map(|s| s.to_string())
                        }
                        TypeKind::Nullable(inner) => match inner.kind() {
                            TypeKind::ClassString(Some(cs_inner)) => {
                                cs_inner.base_name().map(|s| s.to_string())
                            }
                            _ => None,
                        },
                        _ => None,
                    }),
                    _ => None,
                });
                if class_string.is_none() && !resolved.is_empty() {
                    return static_call_on_object(static_call, resolved, ctx);
                }
                class_string
            }
        }
        // `$this->prop::method()`, `getFoo()::method()`: the class is the
        // one a class-string names, or else the class of the object.
        _ => {
            let resolved = resolve_rhs_expression(static_call.class, ctx);
            let class_string = resolved.iter().find_map(|rt| match rt.type_string.kind() {
                TypeKind::ClassString(Some(inner)) => inner.base_name().map(|s| s.to_string()),
                _ => None,
            });
            if class_string.is_none() && !resolved.is_empty() {
                return static_call_on_object(static_call, resolved, ctx);
            }
            class_string
        }
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
            let concrete_owner = crate::type_engine::call_resolution::facade_concrete_owner(
                owner,
                &method_name,
                ctx.class_loader,
                ctx.resolved_class_cache,
                ctx.backend,
            );
            let owner = concrete_owner.as_ref().unwrap_or(owner);
            // `parent::get()` (or `Foo::get()` written out) from inside a
            // class that `@extends Foo<Dog>` reads the parent's `T` as the
            // `Dog` the child bound it to, just as `$this->get()` does.
            let bound_owner = (!owner.template_params.is_empty())
                .then(|| crate::inheritance::extends_type_args(ctx.current_class, owner))
                .flatten()
                .map(|args| {
                    crate::virtual_members::resolve_class_fully_with_type_args(
                        owner,
                        ctx.class_loader,
                        ctx.resolved_class_cache,
                        &args,
                    )
                });
            let owner = bound_owner.as_deref().unwrap_or(owner);

            if let Some(result) = try_resolve_config_method_type(
                &owner.fqn(),
                &method_name,
                &static_call.argument_list,
                ctx,
            ) {
                return result;
            }

            if let Some(result) = try_resolve_trans_method_type(
                &owner.fqn(),
                &method_name,
                &static_call.argument_list,
                ctx,
            ) {
                return result;
            }

            let arg_texts =
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                    &static_call.argument_list,
                    ctx.content,
                );
            let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
            let rctx = ctx.as_resolution_ctx();
            let template_subs =
                Backend::build_method_template_subs(owner, &method_name, &arg_refs, &rctx);
            let owner_key = owner.fqn();
            // An explicit `A::` on a non-static method is PHP's pre-8
            // instance-forwarding form, which keeps `$this` (and with it late
            // static binding) bound, so only a `static` method written out
            // fixes the class. That forwarding needs a `$this` to forward
            // from, so it does not apply outside any class (global-scope
            // code has no enclosing class to bind `static` open over).
            let target_is_static = method_is_static(owner, &method_name, ctx);
            let lsb_class = (!ctx.current_class.name.is_empty()
                && (forwards_lsb || !target_is_static))
                .then(|| ctx.current_class.fqn());
            let self_replace =
                |ty: &PhpType| ty.replace_self_bound(&owner_key, lsb_class.as_deref());

            let mut results = resolve_owner_method_call(
                owner,
                &method_name,
                &static_call.argument_list,
                ctx,
                true,
                &template_subs,
                &self_replace,
            );
            // `Model::factory(…)`, `UserFactory::times(3)` and
            // `UserFactory::new()` open a factory chain, and what they
            // were opened with is what the `create()` at the far end of
            // it builds.  `factory($count)` only settles that once its
            // argument is resolved, which is why the type is fetched
            // lazily rather than for every static call in the file.
            let first_arg_type = || {
                let arg = static_call.argument_list.arguments.first()?;
                let resolved = resolve_rhs_expression(arg.value(), ctx);
                (!resolved.is_empty()).then(|| ResolvedType::types_joined(&resolved))
            };
            crate::virtual_members::laravel::tag_static_factory_call(
                &mut results,
                &method_name,
                arg_refs.first().copied(),
                &first_arg_type,
                &rctx,
            );
            return results;
        }
    }
    vec![]
}

/// `$obj::method()` where `$obj` holds an object rather than a class-string.
///
/// PHP calls the method on the object's own class, so this resolves exactly
/// like `$obj->method()` would: a union receiver unions the results, an
/// intersection intersects them, and `static` binds to the receiver.
fn static_call_on_object(
    static_call: &StaticMethodCall<'_>,
    resolved: Vec<ResolvedType>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let classes = ResolvedType::into_arced_classes(resolved.clone());
    if classes.is_empty() {
        return vec![];
    }
    resolve_member_call_on_receiver(
        static_call.class,
        &static_call.method,
        &static_call.argument_list,
        Some((classes, resolved)),
        true,
        ctx,
    )
}

/// The array shape a Laravel `validated()` / `validate()` /
/// `safe()->only()` call assigns, given the validation rules in scope.
///
/// `object` is the expression the method was called on, which is what tells
/// a `ValidatedInput` receiver apart from the request it came from.
fn try_resolve_validated_shape(
    owner: &ClassInfo,
    call: validated_shape::ShapeCall,
    object: &Expression<'_>,
    arg_refs: &[&str],
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    validated_shape::resolve_shape_at_call(
        owner,
        call,
        arg_refs,
        &|| safe_source_owner(object, ctx),
        ctx.content,
        object.span().end.offset,
        ctx.as_resolution_ctx().class_loader,
        ctx.backend,
    )
}

/// The request class behind a `ValidatedInput` receiver.
///
/// Covers both the direct chain (`$request->safe()->only(…)`) and the
/// two-step form (`$safe = $request->safe(); $safe->only(…)`), which
/// `resolve_var_types`'s assignment tracing already resolves back to the
/// request variable.
fn safe_source_owner(
    object: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    let variable = match object {
        // `$request->safe()->only(…)` — the receiver is the `safe()` call.
        Expression::Call(_) => {
            crate::virtual_members::laravel::safe_call_receiver_variable(object)?
        }
        // `$safe->only(…)` — trace the assignment that produced `$safe`.
        Expression::Variable(Variable::Direct(dv)) => {
            crate::virtual_members::laravel::safe_source_variable(
                ctx.content,
                object.span().end.offset as usize,
                bytes_to_str(dv.name),
            )?
        }
        _ => return None,
    };
    let resolved = resolve_var_types(&variable, ctx, object.span().end.offset);
    ResolvedType::into_arced_classes(resolved)
        .into_iter()
        .next()
}

fn try_resolve_config_method_type(
    owner_fqn: &str,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    const CONFIG_FQNS: &[&str] = &[
        "Illuminate\\Config\\Repository",
        "Illuminate\\Support\\Facades\\Config",
        "Config",
    ];
    if !matches!(method_name, "get" | "array") {
        return None;
    }
    let normalized = owner_fqn.strip_prefix('\\').unwrap_or(owner_fqn);
    if !CONFIG_FQNS
        .iter()
        .any(|fqn| normalized.eq_ignore_ascii_case(fqn))
    {
        return None;
    }
    let resolver = ctx.loaders.config_resolver?;
    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let first_arg = arg_texts.first()?;
    let key = crate::util::unescape_php_string_literal(first_arg.trim())?;
    if key.is_empty() || key.contains('$') {
        return None;
    }
    let ty = resolver(&key)?;
    if method_name == "array" {
        if ty.is_array_like() {
            return Some(vec![ResolvedType::from_type_string(ty)]);
        }
        return None;
    }
    Some(vec![ResolvedType::from_type_string(ty)])
}

/// Narrow `Illuminate\Translation\Translator::get()` (the `Lang::get()`
/// facade method) to `string` when its literal key argument names a
/// scalar translation entry.  Mirrors [`try_resolve_config_method_type`].
fn try_resolve_trans_method_type(
    owner_fqn: &str,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    const TRANS_FQNS: &[&str] = &[
        "Illuminate\\Translation\\Translator",
        "Illuminate\\Support\\Facades\\Lang",
        "Lang",
    ];
    if method_name != "get" {
        return None;
    }
    let normalized = owner_fqn.strip_prefix('\\').unwrap_or(owner_fqn);
    if !TRANS_FQNS
        .iter()
        .any(|fqn| normalized.eq_ignore_ascii_case(fqn))
    {
        return None;
    }
    let resolver = ctx.loaders.trans_resolver?;
    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let first_arg = arg_texts.first()?;
    let ty = crate::util::unescape_php_string_literal(first_arg.trim())
        .filter(|key| !key.is_empty() && !key.contains('$'))
        .and_then(|key| resolver(&key))
        .unwrap_or_else(crate::virtual_members::laravel::unresolved_trans_type);
    Some(vec![ResolvedType::from_type_string(ty)])
}

/// Resolve a request input accessor (`header()`, `query()`, `input()`,
/// `cookie()`, `post()`, `file()`) to what the call's own arguments say it
/// returns, rather than the union that covers every way of calling it.
fn try_resolve_request_accessor_type(
    owner: &ClassInfo,
    accessor: crate::virtual_members::laravel::request_input::InputAccessor,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    arg_refs: &[&str],
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    use crate::virtual_members::laravel::request_input;

    // The accessor is declared on `Illuminate\Http\Request`, while the
    // receiver is usually an app's own `FormRequest` subclass that never
    // redeclares it, so its parameters have to be found by walking the
    // parent chain rather than reading `owner`'s own members.
    let (method, _) = crate::type_engine::types::narrowing::find_method_in_chain_where(
        owner,
        method_name,
        ctx.class_loader,
        &|_| true,
        &mut Vec::new(),
        0,
    )?;
    let bound_text = crate::call_args::bind_text_args_to_params(&method.parameters, arg_refs);
    let bound_exprs = crate::call_args::bind_args_to_params(&method.parameters, argument_list);

    // The default only decides the missing-key branch, so it is resolved
    // only once a keyed call has been established.
    let default_type = || {
        let expr = bound_exprs.get(1).copied().flatten()?;
        let resolved = resolve_rhs_expression(expr, ctx);
        (!resolved.is_empty()).then(|| ResolvedType::types_joined(&resolved))
    };
    let ty = request_input::resolve_accessor_type(
        owner,
        accessor,
        &request_input::AccessorArgs {
            key: bound_text.first().and_then(|k| k.as_deref()),
            default_type: &default_type,
        },
        ctx.content,
        ctx.cursor_offset,
        ctx.class_loader,
        ctx.backend,
    )?;
    // The whole union travels on one entry, carrying the class of its
    // object half: splitting it into an entry per member would leave the
    // array half as a class-less entry that a later `instanceof` guard has
    // no way to rule out.
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &ty,
        &owner.fqn(),
        ctx.all_classes,
        ctx.class_loader,
    );
    Some(vec![match classes.first() {
        Some(class) => ResolvedType::from_both_arc(ty, Arc::clone(class)),
        None => ResolvedType::from_type_string(ty),
    }])
}

/// Narrow `$this->argument('user')` / `$this->option('queue')` on an Artisan
/// command to the type its own `$signature` declares for that parameter.
///
/// The framework's declared union spans every parameter shape at once, so
/// without this a value-less `{--flag}` reads as `array|string|int|bool|null`
/// wherever it is used.
fn try_resolve_command_accessor_type(
    class: &ClassInfo,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !crate::virtual_members::laravel::is_command_accessor(method_name) {
        return None;
    }
    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let key = match arg_texts.first() {
        // A key built at runtime names no parameter we can look up.
        Some(text) => Some(crate::util::unescape_php_string_literal(text.trim())?),
        None => None,
    };
    let ty = crate::virtual_members::laravel::resolve_command_accessor_type(
        class,
        method_name,
        key.as_deref(),
        ctx.class_loader,
        ctx.backend,
    )?;
    Some(vec![ResolvedType::from_type_string(ty)])
}
