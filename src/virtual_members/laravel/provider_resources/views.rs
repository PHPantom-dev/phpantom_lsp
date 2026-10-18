//! The Blade and view registrations a provider makes: component namespaces
//! and directives, view namespaces and directories, and shared view data.

use std::path::{Path, PathBuf};

use mago_syntax::cst::*;

use super::super::const_eval::{Scope, const_string};
use super::super::view_data::{
    ViewDataRegistration, is_view_facade, is_view_factory_expr, view_data_registration,
};
use super::extract::{ProviderFile, names_facade};
use super::{PathContext, ProviderResource, ProviderResources, resolve_path_arg};

/// `Blade::componentNamespace('Nightshade\\Views\\Components',
/// 'nightshade')` says which classes back the views a package registers
/// under its own prefix; the two anonymous registrations point a prefix at a
/// directory of class-less component views.  `Blade::directive()` and
/// `Blade::if()` register directives.  Reports whether the call was one of
/// them.
pub(super) fn record_blade_facade_call(
    file: &ProviderFile<'_, '_>,
    expr: &Expression<'_>,
    resources: &mut ProviderResources,
) -> bool {
    let Expression::Call(Call::StaticMethod(call)) = expr else {
        return false;
    };
    let ClassLikeMemberSelector::Identifier(method) = &call.method else {
        return false;
    };
    if !names_facade(call.class, b"Blade") {
        return false;
    }
    let method = method.value.to_ascii_lowercase();
    record_component_registration(file, &method, &call.argument_list, resources)
        || record_directive_registration(file, &method, &call.argument_list, resources)
}

/// The same registrations written against the compiler instance a deferred
/// callback receives (`$blade->componentNamespace(…)`), which is how a
/// package registers before Blade is resolved.  Reports whether the call
/// was one of them.
pub(super) fn record_blade_compiler_call(
    file: &ProviderFile<'_, '_>,
    method: &[u8],
    call: &MethodCall<'_>,
    resources: &mut ProviderResources,
) -> bool {
    if record_component_registration(file, method, &call.argument_list, resources) {
        return true;
    }
    // `$blade->directive(…)` / `$blade->if(…)`, written the same way.
    // Unlike the component-namespace methods above, these two have
    // names an unrelated fluent API could plausibly carry (`->if()`
    // most of all), so the receiver has to actually name Blade's
    // compiler.
    is_blade_compiler_expr(file, call.object)
        && record_directive_registration(file, method, &call.argument_list, resources)
}

/// `View::addNamespace('webshop', base_path(…))` and the same call written
/// against the container's view factory register a view namespace
/// directly, without `loadViewsFrom()`'s published-copy check.  Reports
/// whether the call was one.
pub(super) fn record_view_namespace(
    file: &ProviderFile<'_, '_>,
    expr: &Expression<'_>,
    resources: &mut ProviderResources,
) -> bool {
    let Some((method, argument_list)) = view_factory_call(expr) else {
        return false;
    };
    let prepend = method.eq_ignore_ascii_case(b"prependnamespace");
    if !prepend && !method.eq_ignore_ascii_case(b"addnamespace") {
        return false;
    }
    let mut args = argument_list.arguments.iter();
    if let Some(ns_arg) = args.next()
        && let Some(namespace) = const_string(ns_arg.value(), file.content, file.scope)
        && let Some(hints_arg) = args.next()
    {
        let registered = view_hint_paths(hints_arg.value(), file.content, &file.paths)
            .into_iter()
            .map(|path| ProviderResource {
                path,
                namespace: namespace.clone(),
            });
        // `FileViewFinder::prependNamespace()` puts its hints ahead of
        // the ones already registered, and the first hint wins.
        if prepend {
            resources.view_dirs.splice(0..0, registered);
        } else {
            resources.view_dirs.extend(registered);
        }
    }
    true
}

/// `View::share()` / `View::composer()` and the same calls written against
/// the container's view factory.  Reports whether the call was one.
pub(super) fn record_view_data(
    file: &ProviderFile<'_, '_>,
    expr: &Expression<'_>,
    resources: &mut ProviderResources,
) -> bool {
    let Some(registration) =
        view_data_registration(expr, file.content, file.path, file.scope, file.resolved)
    else {
        return false;
    };
    match registration {
        ViewDataRegistration::Shared(vars) => resources.shared_view_vars.extend(vars),
        ViewDataRegistration::Composers(composers) => resources.view_composers.extend(composers),
    }
    true
}

/// Record a component-namespace registration, whichever of the three
/// `method` names, and report whether the call was one of them.
///
/// All three are written both on the `Blade` facade and on the compiler
/// instance a deferred callback receives, so both call shapes route here.
fn record_component_registration(
    file: &ProviderFile<'_, '_>,
    method: &[u8],
    argument_list: &ArgumentList<'_>,
    resources: &mut ProviderResources,
) -> bool {
    let (content, scope) = (file.content, file.scope);
    match method {
        b"componentnamespace" => {
            if let Some(entry) = component_namespace_args(argument_list, content, scope) {
                resources.class_component_namespaces.push(entry);
            }
        }
        b"anonymouscomponentnamespace" => {
            if let Some(entry) = anonymous_component_namespace_args(argument_list, content, scope) {
                resources.anonymous_component_namespaces.push(entry);
            }
        }
        b"anonymouscomponentpath" => {
            let mut args = argument_list.arguments.iter();
            if let Some(path_arg) = args.next()
                && let Some(path) = resolve_path_arg(path_arg.value(), content, &file.paths)
            {
                // The prefix is optional: without one, every tag that names
                // no namespace of its own reaches the directory.
                let prefix = args
                    .next()
                    .and_then(|arg| const_string(arg.value(), content, scope))
                    .unwrap_or_default();
                resources.anonymous_component_paths.push((prefix, path));
            }
        }
        _ => return false,
    }
    true
}

/// Record a `Blade::directive('datetime', …)` or `Blade::if('admin', …)`
/// registration, and report whether the call was one of them.
///
/// Only the name matters: the handler is a callback returning whatever PHP
/// it likes, which no static scan can evaluate.  Knowing the directive
/// exists is what stops a template's use of it from being masked as a
/// comment, and a `Blade::if()` name stands for the whole family Blade
/// synthesizes from it.
fn record_directive_registration(
    file: &ProviderFile<'_, '_>,
    method: &[u8],
    argument_list: &ArgumentList<'_>,
    resources: &mut ProviderResources,
) -> bool {
    let conditional = match method {
        b"directive" => false,
        b"if" => true,
        _ => return false,
    };
    // A registration whose name is only known at runtime registers a
    // directive no template can be checked against, so the call is still
    // recognised (nothing else it could be) but records nothing.
    if let Some(first) = argument_list.arguments.iter().next()
        && let Some(name) = const_string(first.value(), file.content, file.scope)
    {
        resources
            .custom_directives
            .push(crate::blade::directives::CustomDirective { name, conditional });
    }
    true
}

/// Whether `expr` names Blade's compiler, i.e. the receiver a provider
/// registers a directive on when it does not go through the facade.
///
/// Covers the `$blade` a `callAfterResolving('blade.compiler', …)` callback
/// receives and the container lookups a provider reaches the compiler by,
/// which is the whole set of shapes Laravel's own documentation and the
/// packages that follow it use.
fn is_blade_compiler_expr(file: &ProviderFile<'_, '_>, expr: &Expression<'_>) -> bool {
    /// The parameter names Laravel's docs and packages give the compiler a
    /// deferred callback receives.
    const COMPILER_VARIABLES: [&[u8]; 3] = [b"$blade", b"$bladeCompiler", b"$compiler"];
    /// The container key the compiler is bound under.
    const COMPILER_KEY: &str = "blade.compiler";

    match expr {
        Expression::Variable(Variable::Direct(dv)) => COMPILER_VARIABLES
            .iter()
            .any(|name| dv.name.eq_ignore_ascii_case(name)),
        // `$this->app['blade.compiler']`
        Expression::ArrayAccess(access) => {
            const_string(access.index, file.content, file.scope).as_deref() == Some(COMPILER_KEY)
        }
        // `app('blade.compiler')` and `$this->app->make(BladeCompiler::class)`
        Expression::Call(Call::Function(fc)) => names_blade_compiler(file, &fc.argument_list),
        Expression::Call(Call::Method(mc)) => names_blade_compiler(file, &mc.argument_list),
        Expression::Parenthesized(inner) => is_blade_compiler_expr(file, inner.expression),
        _ => false,
    }
}

/// Whether a container lookup's first argument names Blade's compiler,
/// either by its container key or by the compiler class itself.
fn names_blade_compiler(file: &ProviderFile<'_, '_>, argument_list: &ArgumentList<'_>) -> bool {
    let Some(first) = argument_list.arguments.iter().next() else {
        return false;
    };
    let named = const_string(first.value(), file.content, file.scope)
        .or_else(|| file.resolved.class_fetch_fqn(first.value()));
    named.is_some_and(|name| {
        name == "blade.compiler" || crate::util::short_name(&name) == "BladeCompiler"
    })
}

/// The (tag prefix, view directory) pair an `anonymousComponentNamespace()`
/// call registers.
///
/// Laravel normalises the directory to the dot notation a view name is
/// written in, and defaults the prefix to the directory as it was written.
fn anonymous_component_namespace_args(
    argument_list: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
) -> Option<(String, String)> {
    let mut args = argument_list.arguments.iter();
    let directory = const_string(args.next()?.value(), content, scope)?;
    let prefix = args
        .next()
        .and_then(|arg| const_string(arg.value(), content, scope))
        .unwrap_or_else(|| directory.clone());
    let directory = directory
        .replace('/', ".")
        .trim_matches(|c| c == '.' || c == ' ')
        .to_string();
    if directory.is_empty() || prefix.is_empty() {
        return None;
    }
    Some((prefix, directory))
}

/// The (tag prefix, class namespace) pair a `componentNamespace()` call
/// registers, as `(prefix, namespace)`.
///
/// The namespace is read from source text, where a single-quoted literal
/// still carries its doubled separators, so those are collapsed back to
/// the namespace the application sees.
fn component_namespace_args(
    argument_list: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
) -> Option<(String, String)> {
    let mut args = argument_list.arguments.iter();
    let namespace = const_string(args.next()?.value(), content, scope)?;
    let prefix = const_string(args.next()?.value(), content, scope)?;
    let namespace = namespace
        .replace("\\\\", "\\")
        .trim_matches('\\')
        .to_string();
    if namespace.is_empty() || prefix.is_empty() {
        return None;
    }
    Some((prefix, namespace))
}

/// The method name and arguments of a call on Laravel's view factory,
/// reached through either the `View` facade or the container.
fn view_factory_call<'ast, 'arena>(
    expr: &'ast Expression<'arena>,
) -> Option<(&'arena [u8], &'ast ArgumentList<'arena>)> {
    match expr {
        Expression::Call(Call::StaticMethod(sc)) if is_view_facade(sc.class) => match &sc.method {
            ClassLikeMemberSelector::Identifier(method) => Some((method.value, &sc.argument_list)),
            _ => None,
        },
        Expression::Call(Call::Method(mc)) if is_view_factory_expr(mc.object) => match &mc.method {
            ClassLikeMemberSelector::Identifier(method) => Some((method.value, &mc.argument_list)),
            _ => None,
        },
        _ => None,
    }
}

/// The directories an `addNamespace()` hint argument names: a single path,
/// or an array of them.
fn view_hint_paths(
    expr: &Expression<'_>,
    content: &str,
    paths: &PathContext<'_, '_>,
) -> Vec<PathBuf> {
    let resolve = |expr: &Expression<'_>| resolve_path_arg(expr, content, paths);
    match expr {
        Expression::Array(array) => array
            .elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::Value(value) => resolve(value.value),
                _ => None,
            })
            .collect(),
        Expression::LegacyArray(array) => array
            .elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::Value(value) => resolve(value.value),
                _ => None,
            })
            .collect(),
        _ => resolve(expr).into_iter().collect(),
    }
}

/// Registers a `loadViewsFrom($path, $namespace)` resource the way
/// `Illuminate\Support\ServiceProvider::loadViewsFrom()` actually resolves
/// it: before adding `$path` itself, it checks each configured view root for
/// a `vendor/$namespace` directory and, if one exists, adds that first.
/// `FileViewFinder` renders the first hint registered for a namespace, so a
/// published copy (created by `artisan vendor:publish`) wins over the
/// package's own directory, and a view only the published directory holds
/// still resolves.
pub(super) fn push_view_dir_registration(
    resources: &mut ProviderResources,
    path: PathBuf,
    namespace: String,
    workspace_root: &Path,
) {
    for root in crate::blade::discover_view_paths(workspace_root) {
        let published = root.join("vendor").join(&namespace);
        if published.is_dir() {
            resources.view_dirs.push(ProviderResource {
                path: published,
                namespace: namespace.clone(),
            });
        }
    }
    resources
        .view_dirs
        .push(ProviderResource { path, namespace });
}
