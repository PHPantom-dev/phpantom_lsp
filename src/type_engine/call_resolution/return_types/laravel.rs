//! Laravel-specific call return types.
//!
//! Every function here refines what a call resolves to for one Laravel
//! API (the container, auth guards, request input accessors, validated
//! shapes, views, and the configured date class); the generic resolution
//! in the parent module answers everything else.

use std::sync::Arc;

use crate::Backend;
use crate::atom::atom;
use crate::class_lookup::Fqn;
use crate::php_type::{PhpType, TypeKind};
use crate::text_scan::split_top_level_commas;
use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, ClassInfo, ResolvedType};

/// The class a Laravel helper function call returns when it is more
/// specific than the declared return type: a container string binding,
/// the configured date class, the concrete view, or the response a
/// `response()` call with content builds.
pub(super) fn laravel_helper_call_class(
    func_name: &str,
    text_args: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    // ── Laravel container string binding ────────────────
    // `app('blade.compiler')` / `resolve('cache')` bind a plain string to a
    // concrete class.  The class-string form (`app(User::class)`) is
    // handled by the conditional return type; only a literal string
    // binding is intercepted here, resolved via the framework's own alias
    // table.
    let normalized_func = func_name.trim_start_matches('\\');
    if matches!(normalized_func, "app" | "resolve")
        && let Some(binding) = Backend::extract_first_arg_text(text_args)
        && let Some(name) = crate::util::unescape_php_string_literal(binding.trim())
        && let Some(cls) = (ctx.file.class_loader)(Fqn::new(&name))
    {
        return Some(cls);
    }

    // ── now() / today() → configured Laravel date class ──
    // The global `now()`/`today()` helpers are declared to return
    // `CarbonInterface`, but they actually instantiate the concrete class
    // selected by Laravel's date factory. Resolving to the interface loses
    // the concrete type and produces spurious mismatches when a chained
    // call is assigned to a `DateTime`/`DateTimeImmutable` declaration.
    // Map both to the concrete class.  Only applies when the class is
    // loadable (i.e. inside a Laravel project).
    //
    // Not strictly sound (the declared type is the interface), but it is
    // what the Laravel PHPStan extensions infer too, and the ecosystem is
    // written against it.  See the matching note in `rhs_resolution.rs`.
    if matches!(
        normalized_func,
        "now" | "today" | "Illuminate\\Support\\now" | "Illuminate\\Support\\today"
    ) && let Some(cls) = (ctx.file.class_loader)(Fqn::new(
        crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN,
    )) {
        return Some(cls);
    }

    // ── view('name') → concrete Illuminate\View\View ─────
    if crate::virtual_members::laravel::view_helper_returns_view(
        normalized_func,
        Backend::extract_first_arg_text(text_args)
            .as_deref()
            .unwrap_or(""),
    ) && let Some(cls) =
        (ctx.file.class_loader)(Fqn::new(crate::virtual_members::laravel::VIEW_FQN))
    {
        return Some(cls);
    }

    // ── response($content) → Illuminate\Http\Response ────
    if crate::virtual_members::laravel::response_helper_returns_response(normalized_func, text_args)
        && let Some(cls) =
            (ctx.file.class_loader)(Fqn::new(crate::virtual_members::laravel::RESPONSE_FQN))
    {
        return Some(cls);
    }
    None
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
pub(super) fn resolve_auth_user_at_call(
    base: &SubjectExpr,
    user_args: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<Arc<ClassInfo>>> {
    // Cheap gate first: without the server state there is nothing to
    // refine, so skip the (comparatively expensive) subtype walk below.
    let backend = ctx.file.backend?;

    // Only intercept `user()` on an actual auth entry point.  Every
    // other class with a `user()` method must resolve normally.
    let is_auth_receiver = owners.iter().any(|rt| {
        rt.class_info.as_ref().is_some_and(|ci| {
            crate::class_lookup::is_subtype_of(
                ci,
                crate::virtual_members::laravel::GUARD_FQN,
                ctx.file.class_loader,
            ) || crate::class_lookup::is_subtype_of(
                ci,
                crate::virtual_members::laravel::REQUEST_FQN,
                ctx.file.class_loader,
            )
        })
    });
    if !is_auth_receiver {
        return None;
    }

    let guard = auth_guard_name(base, user_args);
    let loader = |name: Fqn<'_>| backend.find_or_load_class(&name);
    let model_type = crate::virtual_members::laravel::resolve_auth_user_type(
        backend,
        guard.as_deref(),
        &loader,
    )?;

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed_returned(
        &model_type,
        "",
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    if classes.is_empty() {
        None
    } else {
        Some(classes)
    }
}

/// The type a request input accessor call returns, given the arguments it
/// was written with.
///
/// The subject-expression path reaches the arguments as text, which is all
/// the key needs; the default's type is resolved through the shared
/// pipeline the same way an argument anywhere else is.
pub(super) fn resolve_request_accessor_at_call(
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
    let (method, _) = crate::type_engine::type_ops::condition_facts::find_method_in_chain_where(
        receiver,
        method_name,
        ctx.file.class_loader,
        &|_| true,
        &mut Vec::new(),
        0,
    )?;
    let args = split_top_level_commas(text_args);
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
        ctx.file.content,
        ctx.cursor_offset,
        ctx.file.class_loader,
        ctx.file.backend,
    )
}

/// The array shape a Laravel `validated()` / `validate()` /
/// `safe()->only()` call returns, given the rules in scope at the call site.
///
/// Returns `None` for every other call, leaving the declared return type
/// alone.
pub(super) fn resolve_validated_shape_at_call(
    base: &SubjectExpr,
    method_name: &str,
    text_args: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    use crate::virtual_members::laravel::validated_shape;

    let call = validated_shape::shape_bearing_method(method_name)?;
    let receiver = owners.iter().find_map(|rt| rt.class_info.as_ref())?;
    let mut args = split_top_level_commas(text_args);
    for arg in &mut args {
        *arg = crate::call_args::text_arg_value(arg);
    }

    validated_shape::resolve_shape_at_call(
        receiver,
        call,
        &args,
        &|| validated_shape::safe_source_class(base, ctx),
        ctx.file.content,
        ctx.cursor_offset,
        ctx.file.class_loader,
        ctx.file.backend,
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
    let first = split_top_level_commas(args_text).into_iter().next()?;
    crate::text_scan::unquote_php_string(crate::call_args::text_arg_value(first))
        .map(str::to_string)
}

pub(super) fn replace_support_carbon_return(
    ty: &PhpType,
    configured_class: &str,
) -> Option<PhpType> {
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

#[cfg(test)]
mod auth_guard_tests {
    use super::{
        auth_guard_name, first_string_literal_arg, replace_support_carbon_return,
        resolve_validated_shape_at_call,
    };
    use crate::Backend;
    use crate::atom::atom;
    use crate::class_lookup::Fqn;
    use crate::php_type::PhpType;
    use crate::test_fixtures::{make_class, make_method};
    use crate::type_engine::resolver::{FileCtx, ResolutionCtx};
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
        let class_loader = |name: Fqn<'_>| {
            let name = name.as_str();
            (name.trim_start_matches('\\') == "Illuminate\\Http\\Request")
                .then(|| Arc::clone(&request))
        };
        assert!(class_loader(Fqn::new("\\Illuminate\\Http\\Request")).is_some());
        let ctx = ResolutionCtx::new(
            FileCtx::new(
                "",
                &classes,
                &class_loader,
                crate::test_fixtures::fqn_source_loader(&class_loader),
            ),
            None,
            0,
        );
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
        let loader = |name: Fqn<'_>| {
            let name = name.as_str();
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
        let loader = |name: Fqn<'_>| {
            let name = name.as_str();
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
