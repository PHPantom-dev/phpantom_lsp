//! Laravel-specific call return types.
//!
//! Every function here refines what a call resolves to for one Laravel
//! API (the auth guards, request input accessors, validated shapes, and
//! the configured date class); the generic resolution in the parent
//! module answers everything else.

use std::sync::Arc;

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::conditional_resolution::split_text_args;
use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, ClassInfo, ResolvedType};

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
