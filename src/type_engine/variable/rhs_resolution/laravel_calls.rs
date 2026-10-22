//! Laravel calls whose arguments decide more than the declared return
//! type says: helper functions (`app()`, `now()`, `view()`, `config()`,
//! `trans()`), configuration and translation lookups through their
//! facades, validated request input, and request and console accessors.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::class_lookup::Fqn;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};
use crate::virtual_members::laravel::validated_shape;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast;

use super::{resolve_rhs_expression, resolve_var_types};

/// What the Laravel helper function `name` returns when its arguments
/// decide more than its declared return type: the class a container
/// binding names, the configured date class, the concrete view, a
/// configuration value, or a translation.
pub(super) fn laravel_helper_call(
    name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let normalized_func = name.trim_start_matches('\\');

    // ── Laravel container string binding ────────────────
    // `$var = app('blade.compiler')` / `$var = resolve('cache')` bind a
    // plain string to a concrete class via the framework's container
    // alias table. Mirrors the direct-call-subject interception in
    // call_resolution.rs so the binding survives being assigned to a
    // variable instead of being chained off the call directly.
    if matches!(normalized_func, "app" | "resolve") {
        let arg_texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
        if let Some(first_arg) = arg_texts.first()
            && let Some(alias) = crate::util::unescape_php_string_literal(first_arg.trim())
            && let Some(cls) = (ctx.file.class_loader)(Fqn::new(&alias))
        {
            return Some(ResolvedType::from_classes(vec![cls]));
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
    ) && let Some(cls) = (ctx.file.class_loader)(Fqn::new(
        crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN,
    )) {
        return Some(ResolvedType::from_classes(vec![cls]));
    }

    // ── view('name') → concrete Illuminate\View\View ──
    // The helper's conditional return type names the *contract*, but
    // the factory always builds the concrete view object.  Mirrors the
    // `view()` stub the Laravel PHPStan extensions ship.
    if normalized_func == "view" {
        let arg_texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
        if crate::virtual_members::laravel::view_helper_returns_view(
            normalized_func,
            arg_texts.first().map(String::as_str).unwrap_or(""),
        ) && let Some(cls) =
            (ctx.file.class_loader)(Fqn::new(crate::virtual_members::laravel::VIEW_FQN))
        {
            return Some(ResolvedType::from_classes(vec![cls]));
        }
    }

    // ── config() return type inference ───────
    if matches!(normalized_func, "config" | "Illuminate\\Support\\config")
        && let Some(ty) = config_value_type(argument_list, ctx)
    {
        return Some(vec![ResolvedType::from_type_string(ty)]);
    }

    // ── Translation helper return type narrowing ─────
    // `trans()`/`__()` declare a return type of `string|array|null` because
    // a translation key may name a whole group, and the keyless form hands
    // the key straight back. The key decides which branch a call takes:
    // a leaf entry is a `string`, a group is the array beneath it, and
    // `__()` with no key at all is the `null`.
    if matches!(normalized_func, "trans" | "__")
        && let Some(resolver) = ctx.loaders.trans_resolver
    {
        let arg_texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
        match arg_texts.first() {
            Some(first_arg) => {
                let ty = translation_type(first_arg, resolver);
                return Some(vec![ResolvedType::from_type_string(ty)]);
            }
            // `trans()` with no key hands back the translator itself, a
            // shape this narrowing does not model; `__()` returns the
            // null it was given.
            None if normalized_func == "__" => {
                return Some(vec![ResolvedType::from_type_string(PhpType::null())]);
            }
            None => {}
        }
    }

    None
}

/// The type of the configuration value a call's first argument names by
/// a literal key, as `config('app.name')` and `Config::get('app.name')`
/// read it.
fn config_value_type(
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolver = ctx.loaders.config_resolver?;
    let arg_texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
    let key = crate::util::unescape_php_string_literal(arg_texts.first()?.trim())?;
    if key.is_empty() || key.contains('$') {
        return None;
    }
    resolver(&key)
}

/// The type of the translation whose key is written as `key_text`: the
/// entry's own type for a literal key the catalog knows, and the type any
/// translation can have otherwise.
fn translation_type(key_text: &str, resolver: &dyn Fn(&str) -> Option<PhpType>) -> PhpType {
    crate::util::unescape_php_string_literal(key_text.trim())
        .filter(|key| !key.is_empty() && !key.contains('$'))
        .and_then(|key| resolver(&key))
        .unwrap_or_else(crate::virtual_members::laravel::unresolved_trans_type)
}

/// Narrow `Illuminate\Config\Repository::get()` / `array()` (and the
/// `Config` facade) to the type of the configuration value its literal key
/// names.
pub(super) fn try_resolve_config_method_type(
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
    let ty = config_value_type(argument_list, ctx)?;
    if method_name == "array" && !ty.is_array_like() {
        return None;
    }
    Some(vec![ResolvedType::from_type_string(ty)])
}

/// Narrow `Illuminate\Translation\Translator::get()` (the `Lang::get()`
/// facade method) to `string` when its literal key argument names a
/// scalar translation entry.  Mirrors [`try_resolve_config_method_type`].
pub(super) fn try_resolve_trans_method_type(
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
    let arg_texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
    let ty = translation_type(arg_texts.first()?, resolver);
    Some(vec![ResolvedType::from_type_string(ty)])
}

/// The array shape a Laravel `validated()` / `validate()` /
/// `safe()->only()` call assigns, given the validation rules in scope.
///
/// `object` is the expression the method was called on, which is what tells
/// a `ValidatedInput` receiver apart from the request it came from.
pub(super) fn try_resolve_validated_shape(
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
        ctx.file.content,
        object.span().end.offset,
        ctx.as_resolution_ctx().file.class_loader,
        ctx.file.backend,
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
                ctx.file.content,
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

/// Resolve a request input accessor (`header()`, `query()`, `input()`,
/// `cookie()`, `post()`, `file()`) to what the call's own arguments say it
/// returns, rather than the union that covers every way of calling it.
pub(super) fn try_resolve_request_accessor_type(
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
    let (method, _) = crate::type_engine::type_ops::condition_facts::find_method_in_chain_where(
        owner,
        method_name,
        ctx.file.class_loader,
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
        ctx.file.content,
        ctx.cursor_offset,
        ctx.file.class_loader,
        ctx.file.backend,
    )?;
    // The whole union travels on one entry, carrying the class of its
    // object half: splitting it into an entry per member would leave the
    // array half as a class-less entry that a later `instanceof` guard has
    // no way to rule out.
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &ty,
        &owner.fqn(),
        ctx.file.all_classes,
        ctx.file.class_loader,
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
pub(super) fn try_resolve_command_accessor_type(
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
        ctx.file.content,
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
        ctx.file.class_loader,
        ctx.file.backend,
    )?;
    Some(vec![ResolvedType::from_type_string(ty)])
}
