//! Call return-type resolution: the dispatcher over the three call forms
//! (resolved in [`function_calls`](super::function_calls),
//! [`method_calls`](super::method_calls) and
//! [`static_calls`](super::static_calls)), function-level `@template`
//! substitution, the helpers that read a call argument's raw type for
//! template binding, and the type a closure literal produces.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{atom, bytes_to_str};
use crate::php_type::{CallableParam, PhpType, TypeKind};
use crate::types::ResolvedType;

use crate::type_engine::resolver::VarResolutionCtx;

use super::function_calls::{
    callable_invocation, first_class_function_return_type, partial_application_return,
    resolve_rhs_function_call,
};
use super::method_calls::{
    resolve_member_call_on_receiver, resolve_method_receiver, resolve_rhs_method_call_inner,
    selected_member_name,
};
use super::static_calls::{StaticCallParts, resolve_rhs_static_call};
use super::{resolve_rhs_expression, resolve_var_types, resolved_type_with_lookup};

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
                if let Some(hint) = crate::inheritance::resolve_property_type_hint(
                    cls,
                    prop,
                    rctx.file.class_loader,
                ) {
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

    // 1. When the diagnostic scope cache is active (and not still being
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

    let resolved = rctx.resolve_variable_at_cursor(var_name);
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
/// (`resolve_call_return_type_hint`) so class-level and
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
    Backend::resolve_call_return_type_hint(callee, args_text, rctx)
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
        Call::StaticMethod(static_call) => {
            resolve_rhs_static_call(StaticCallParts::of_call(static_call), ctx)
        }
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

/// The values a call whose return type is `ty` produces.
///
/// Prefers class-backed results when the type names concrete classes, keeping
/// the full type string as the hint (so generics like `Collection<int, User>`
/// survive).  When the type names no class (a bare `array<…>`, `list<…>`,
/// scalar, or shape) a type-string-only entry is returned so consumers that
/// read `.type_string` still see it.
pub(super) fn resolve_return_type(ty: PhpType, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    let current_class_name = ctx.current_class.fqn();
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &ty,
        &current_class_name,
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    if !classes.is_empty() {
        return ResolvedType::from_classes_with_hint(classes, ty);
    }
    vec![resolved_type_with_lookup(
        ty,
        ctx.file.all_classes,
        ctx.file.class_loader,
    )]
}

/// The `Generator` a closure or arrow function literal returns when its
/// body yields: its keys and values from its own `yield`s, and its return
/// from its own `return`s.  `None` when the body does not yield.
fn closure_generator_type(expr: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> Option<PhpType> {
    let mut found = false;
    let mut keys: Vec<PhpType> = Vec::new();
    let mut values: Vec<PhpType> = Vec::new();
    // Nothing declares these types, so a literal stands for its kind
    // (`yield 1` makes a generator of `int`).
    let resolved = |expr: &Expression<'_>| {
        let types = resolve_rhs_expression(expr, ctx);
        if types.is_empty() {
            PhpType::mixed()
        } else {
            ResolvedType::types_joined(&types).widen_scalar_literals()
        }
    };
    let mut visit = |yield_expr: &Yield<'_>| {
        found = true;
        match yield_expr {
            // A bare `yield $value` is keyed by position.
            Yield::Value(value) => {
                keys.push(PhpType::int());
                values.push(value.value.map_or_else(PhpType::null, resolved));
            }
            Yield::Pair(pair) => {
                keys.push(resolved(pair.key));
                values.push(resolved(pair.value));
            }
            // `yield from` delegates to another iterator.
            Yield::From(_) => {}
        }
    };
    let returns = match expr {
        Expression::Closure(closure) => {
            crate::parser::for_each_own_yield(closure.body.statements.iter(), &mut visit);
            let mut returns = Vec::new();
            crate::return_collection::collect_returns(closure.body.statements.iter(), &mut returns);
            returns
        }
        Expression::ArrowFunction(arrow) => {
            crate::parser::for_each_own_yield_in_expression(arrow.expression, &mut visit);
            Vec::new()
        }
        _ => return None,
    };
    if !found {
        return None;
    }
    let returned: Vec<PhpType> = returns
        .iter()
        .map(|(value, ..)| value.map_or_else(PhpType::void, resolved))
        .collect();
    let join = |types: Vec<PhpType>, empty: PhpType| match types.len() {
        0 => empty,
        1 => types.into_iter().next().unwrap_or(empty),
        _ => PhpType::union(types),
    };
    Some(PhpType::generic(
        "Generator",
        vec![
            join(keys, PhpType::int()),
            join(values, PhpType::mixed()),
            PhpType::mixed(),
            join(returned, PhpType::void()),
        ],
    ))
}

pub(crate) fn infer_closure_literal_type(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    // `$fn(...)` wraps a callable value rather than naming a function.
    if let Expression::PartialApplication(PartialApplication::Function(fpa)) = expr
        && !matches!(fpa.function, Expression::Identifier(_))
    {
        return closure_from_callable(fpa.function, ctx);
    }

    // The declared `: ReturnType`, else the `Generator` the body's yields
    // make the closure return.  Both name their classes in full already.
    let explicit_or_yield = extract_closure_or_arrow_return_type(expr, ctx)
        .or_else(|| closure_generator_type(expr, ctx));

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
        // against its own signature the way a call to it would. Each seed
        // carries the classes its hint names, so a member access on the
        // parameter (`$u->label()`) has an owner to look the member up on.
        let param_types: HashMap<String, Vec<ResolvedType>> = parameter_list
            .parameters
            .iter()
            .filter_map(|param| {
                let hint = param.hint.as_ref()?;
                let name = bytes_to_str(param.variable.name).to_string();
                let ty = ctx
                    .written_names()
                    .resolve_hint(&crate::parser::extract_hint_type(hint));
                Some((name, resolve_return_type(ty, ctx)))
            })
            .collect();

        let resolved = if param_types.is_empty() {
            resolve_rhs_expression(ret_expr, ctx)
        } else {
            let param_aware_resolver = |name: &str| -> Vec<ResolvedType> {
                match param_types.get(name) {
                    Some(seeded) => seeded.clone(),
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
            let narrowed = body_return().filter(|body| {
                crate::class_lookup::is_subtype_of_typed(body, &declared, ctx.file.class_loader)
            });
            Some(narrowed.unwrap_or(declared))
        }
        None => body_return().or_else(|| match expr {
            // First-class callable syntax: `strlen(...)`,
            // `$this->method(...)`, `ClassName::method(...)` and
            // `$invokable(...)` return what calling the callable does,
            // and a partial application what the arguments it binds
            // decide.
            Expression::PartialApplication(pa) => partial_application_return(pa, ctx)
                .filter(|resolved| !resolved.is_empty())
                .map(|resolved| ResolvedType::types_joined(&resolved))
                .or_else(|| first_class_callable_return_type(pa, expr, ctx)),
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

    let params = match expr {
        Expression::PartialApplication(pa) => partial_application_params(pa, ctx),
        _ => declared_closure_params(expr, ctx),
    };
    if inferred_return.is_some() || !params.is_empty() {
        PhpType::callable_spec("Closure", params, inferred_return)
    } else {
        PhpType::closure()
    }
}

/// What calling the first-class callable `pa` (written as `at`) returns:
/// the method it names, called with no arguments to decide anything, or
/// what invoking the value it wraps returns.
pub(crate) fn first_class_callable_return_type<'b>(
    pa: &'b PartialApplication<'b>,
    at: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolved = match pa {
        PartialApplication::Method(ma) => {
            resolve_member_call_on_receiver(ma.object, &ma.method, None, None, false, ctx)
        }
        PartialApplication::StaticMethod(sma) => resolve_rhs_static_call(
            StaticCallParts {
                class: sma.class,
                method: &sma.method,
                argument_list: None,
            },
            ctx,
        ),
        PartialApplication::Function(fa) if matches!(fa.function, Expression::Identifier(_)) => {
            return first_class_function_return_type(fa, ctx);
        }
        PartialApplication::Function(fa) => callable_invocation(fa.function, at, ctx),
    };
    (!resolved.is_empty()).then(|| ResolvedType::types_joined(&resolved))
}

/// The closure `Closure::fromCallable($value)` and `$value(...)` make from
/// the callable `value`: a closure literal unchanged, a value whose type
/// carries a callable signature with that signature, and otherwise a
/// closure returning what invoking the value does (an array callable, a
/// callable string, an object with `__invoke()`).
pub(crate) fn closure_from_callable(value: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> PhpType {
    let value = crate::parser::unwrap_parens(value);
    if matches!(
        value,
        Expression::Closure(_) | Expression::ArrowFunction(_) | Expression::PartialApplication(_)
    ) {
        return infer_closure_literal_type(value, ctx);
    }
    let resolved = match value {
        Expression::Variable(Variable::Direct(dv)) => {
            resolve_var_types(bytes_to_str(dv.name), ctx, ctx.cursor_offset)
        }
        _ => resolve_rhs_expression(value, ctx),
    };
    if let TypeKind::Callable(callable) = ResolvedType::types_joined(&resolved).kind() {
        return PhpType::callable_type(crate::php_type::CallableType {
            kind: atom("Closure"),
            ..(**callable).clone()
        });
    }
    let returned = super::function_calls::value_invocation(&resolved, ctx);
    if returned.is_empty() {
        PhpType::closure()
    } else {
        PhpType::callable_spec(
            "Closure",
            Vec::new(),
            Some(ResolvedType::types_joined(&returned)),
        )
    }
}

/// The parameters a call to the closure the partial application `pa` makes
/// takes: those of the method or function it wraps, narrowed to the ones its
/// placeholders leave open.  `$r->make(1, ?)` over `make(int $id, string
/// $class)` takes a `string`, and `$r->find(...)` everything `find()` does.
///
/// Empty when the wrapped callable cannot be found.
fn partial_application_params(
    pa: &PartialApplication<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<CallableParam> {
    let rctx = ctx.as_resolution_ctx();
    let declared = match pa {
        PartialApplication::Function(fa) => match fa.function {
            Expression::Identifier(ident) => ctx
                .function_loader()
                .and_then(|load| load(bytes_to_str(ident.value()), ident.span().start.offset))
                .map(|func| func.parameters),
            _ => None,
        },
        PartialApplication::Method(ma) => selected_member_name(&ma.method, ctx).and_then(|name| {
            let (classes, resolved) = resolve_method_receiver(ma.object, ctx);
            // `$this` resolves to its classes alone.
            let receivers = if resolved.is_empty() {
                classes.into_iter().map(ResolvedType::from_arc).collect()
            } else {
                resolved
            };
            receivers
                .iter()
                .find_map(|rt| Backend::method_target_on_receiver(rt, &name, &rctx, None))
                .map(|target| target.parameters)
        }),
        PartialApplication::StaticMethod(sma) => {
            selected_member_name(&sma.method, ctx).and_then(|name| {
                let span = sma.class.span();
                let class = ctx
                    .file
                    .content
                    .get(span.start.offset as usize..span.end.offset as usize)?;
                Backend::resolve_static_method_callable(class, &name, &rctx, None)
                    .map(|target| target.parameters)
            })
        }
    };
    let Some(declared) = declared else {
        return Vec::new();
    };
    let to_param = |param: &crate::types::ParameterInfo| CallableParam {
        type_hint: param.type_hint.clone().unwrap_or_else(PhpType::mixed),
        optional: !param.is_required,
        variadic: param.is_variadic,
    };
    if pa.is_first_class_callable() {
        return declared.iter().map(to_param).collect();
    }
    let list = pa.get_argument_list();
    let bound = ctx
        .file
        .content
        .get(
            list.left_parenthesis.end.offset as usize..list.right_parenthesis.start.offset as usize,
        )
        .unwrap_or("");
    crate::call_args::partial_application_params(&declared, bound)
        .iter()
        .map(to_param)
        .collect()
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
                    ctx.written_names()
                        .resolve_hint(&crate::parser::extract_hint_type(hint))
                })
                .unwrap_or_else(PhpType::mixed),
            optional: param.default_value.is_some(),
            variadic: param.ellipsis.is_some(),
        })
        .collect()
}

/// The return type a closure or arrow function literal declares, e.g.
/// `Foo` for `fn (): Foo => …`, qualified against the file's imports.
///
/// `None` for any other expression, or a literal with no return type.
pub(super) fn extract_closure_or_arrow_return_type(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let hint = match expr {
        Expression::ArrowFunction(arrow) => arrow.return_type_hint.as_ref()?,
        Expression::Closure(closure) => closure.return_type_hint.as_ref()?,
        _ => return None,
    };
    Some(
        ctx.written_names()
            .resolve_hint(&crate::parser::extract_hint_type(&hint.hint)),
    )
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
                .file
                .content
                .get(span.start.offset as usize..span.end.offset as usize)?;
            if written.trim() != wanted {
                return None;
            }
            crate::type_engine::variable::resolution::resolve_arg_raw_type(value, ctx)
        })
    }
}
