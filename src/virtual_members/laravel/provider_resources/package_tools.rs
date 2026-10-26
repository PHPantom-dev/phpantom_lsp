//! The resources a `spatie/laravel-package-tools` provider registers through
//! the `Package` it configures.

use std::path::{Path, PathBuf};

use mago_syntax::cst::*;

use super::super::const_eval::{Scope, const_string};
use super::extract::ProviderFile;
use super::views::push_view_dir_registration;
use super::{ProviderResource, ProviderResources};

/// How many `->` links a `$package->name(…)->hasTranslations()->hasViews()
/// ->…` registration may put between the `Package` variable and the call
/// being resolved. Every `has*()` a provider chains onto one `name()` call
/// adds one link, and real providers chain at most a handful.
const PACKAGE_TOOLS_CHAIN_DEPTH: usize = 8;

/// The `Package` variable name `configurePackage(Package $package)`
/// conventionally declares, matched by name the way [`is_blade_compiler_expr`]
/// matches the compiler variable: every `spatie/laravel-package-tools`
/// provider, including every one spatie itself ships, follows this
/// convention.
const PACKAGE_TOOLS_VARIABLE: &[u8] = b"$package";

/// `Package` methods that register a resource under the package's short
/// name, unless given an explicit name/namespace of their own.
const PACKAGE_TOOLS_RESOURCE_METHODS: [&[u8]; 3] =
    [b"hastranslations", b"hasviews", b"hasconfigfile"];

/// `$package->name('laravel-billing')->hasTranslations()` and its
/// `hasViews`/`hasConfigFile` siblings, written from
/// `configurePackage(Package $package)` (`spatie/laravel-package-tools`).
/// Reports whether the call was one of them.
pub(super) fn record_package_call(
    file: &ProviderFile<'_, '_>,
    method: &[u8],
    call: &MethodCall<'_>,
    resources: &mut ProviderResources,
) -> bool {
    if !PACKAGE_TOOLS_RESOURCE_METHODS.contains(&method)
        || !chain_roots_at_package_var(call.object, PACKAGE_TOOLS_CHAIN_DEPTH)
    {
        return false;
    }
    let Some(name) = package_tools_name(
        call.object,
        PACKAGE_TOOLS_CHAIN_DEPTH,
        file.content,
        file.scope,
    ) else {
        // A config file named after a package name we cannot read lives
        // under a root we cannot name.
        if method == b"hasconfigfile" {
            resources.open_config_namespace = true;
            return true;
        }
        return false;
    };
    record_package_tools_registration(
        file,
        method,
        &call.argument_list,
        &PackageToolsContext {
            short_name: package_tools_short_name(&name),
            package_base_dir: &package_tools_base_dir(file.path),
        },
        resources,
    );
    true
}

/// Walk at most `depth` links down a method chain to check whether it
/// bottoms out at the `$package` variable [`PACKAGE_TOOLS_VARIABLE`] names.
fn chain_roots_at_package_var(expr: &Expression<'_>, depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            dv.name.eq_ignore_ascii_case(PACKAGE_TOOLS_VARIABLE)
        }
        Expression::Call(Call::Method(mc)) => chain_roots_at_package_var(mc.object, depth - 1),
        _ => false,
    }
}

/// Walk down a `$package->name('laravel-billing')->hasConfigFile()->…`
/// chain looking for the literal name `Package::name()` was given.
fn package_tools_name(
    expr: &Expression<'_>,
    depth: usize,
    content: &str,
    scope: &Scope,
) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let Expression::Call(Call::Method(mc)) = expr else {
        return None;
    };
    let ClassLikeMemberSelector::Identifier(method) = &mc.method else {
        return None;
    };
    if method.value.eq_ignore_ascii_case(b"name")
        && let Some(first) = mc.argument_list.arguments.iter().next()
    {
        return const_string(first.value(), content, scope);
    }
    package_tools_name(mc.object, depth - 1, content, scope)
}

/// `Illuminate\Support\Str::after($name, 'laravel-')`: `Package::shortName()`
/// applied to whatever `name()` was given, dropping everything up to and
/// including the first `laravel-`. A name without that substring is left
/// unchanged, the way `Str::after` leaves any string it cannot find alone.
fn package_tools_short_name(name: &str) -> &str {
    match name.find("laravel-") {
        Some(idx) => &name[idx + "laravel-".len()..],
        None => name,
    }
}

/// `PackageServiceProvider::getPackageBaseDir()`: the provider file's own
/// directory, moved up one level when the file sits directly in a
/// `Providers` folder (packages that mirror Laravel's own app structure put
/// their provider there, one level below the package root).
fn package_tools_base_dir(file_path: &Path) -> PathBuf {
    let dir = file_path.parent().unwrap_or(file_path);
    match dir.file_name().and_then(|name| name.to_str()) {
        Some("Providers") => dir.parent().unwrap_or(dir).to_path_buf(),
        _ => dir.to_path_buf(),
    }
}

/// The context a `$package->has*()` registration (see
/// [`record_package_tools_registration`]) reads its default namespace and
/// resource directory from.
struct PackageToolsContext<'a> {
    short_name: &'a str,
    package_base_dir: &'a Path,
}

/// `$package->hasTranslations()`, `->hasViews($namespace = null)`, and
/// `->hasConfigFile($name = null)`: the resource registrations
/// `spatie/laravel-package-tools` exposes on `Package`. Each defaults its
/// namespace/config key to `short_name` and reads its directory from
/// `<package_base_dir>/../resources/<kind>` (`../config/<name>.php` for a
/// config file), exactly as `PackageServiceProvider`'s own `boot*()`/
/// `register()` methods do.
fn record_package_tools_registration(
    file: &ProviderFile<'_, '_>,
    method: &[u8],
    argument_list: &ArgumentList<'_>,
    ctx: &PackageToolsContext<'_>,
    resources: &mut ProviderResources,
) {
    let first_arg_name = argument_list
        .arguments
        .iter()
        .next()
        .and_then(|arg| const_string(arg.value(), file.content, file.scope));

    match method {
        b"hastranslations" => {
            let resolved = ctx
                .package_base_dir
                .join("..")
                .join("resources")
                .join("lang");
            resources.trans_dirs.push(ProviderResource {
                path: resolved.canonicalize().unwrap_or(resolved),
                namespace: ctx.short_name.to_string(),
            });
        }
        b"hasviews" => {
            let resolved = ctx
                .package_base_dir
                .join("..")
                .join("resources")
                .join("views");
            let namespace = first_arg_name.unwrap_or_else(|| ctx.short_name.to_string());
            push_view_dir_registration(
                resources,
                resolved.canonicalize().unwrap_or(resolved),
                namespace,
                file.paths.workspace_root,
            );
        }
        b"hasconfigfile" => {
            let config_names = match argument_list.arguments.iter().next() {
                None => Some(vec![ctx.short_name.to_string()]),
                Some(arg) => match (first_arg_name, arg.value()) {
                    (Some(name), _) => Some(vec![name]),
                    (None, Expression::Array(array)) => {
                        const_string_list(array.elements.iter(), file)
                    }
                    (None, Expression::LegacyArray(array)) => {
                        const_string_list(array.elements.iter(), file)
                    }
                    (None, _) => None,
                },
            };
            // A name computed at runtime registers a config file this scan
            // cannot name, so any config root may be one it registered.
            let Some(config_names) = config_names else {
                resources.open_config_namespace = true;
                return;
            };
            for config_name in config_names {
                let resolved = ctx
                    .package_base_dir
                    .join("..")
                    .join("config")
                    .join(format!("{config_name}.php"));
                resources.config_files.push(ProviderResource {
                    path: resolved.canonicalize().unwrap_or(resolved),
                    namespace: config_name.replace(['/', '\\'], "."),
                });
            }
        }
        _ => {}
    }
}

/// The names an array literal lists, or `None` when any element is not a
/// constant string.
fn const_string_list<'a, 'arena: 'a>(
    elements: impl Iterator<Item = &'a ArrayElement<'arena>>,
    file: &ProviderFile<'_, '_>,
) -> Option<Vec<String>> {
    elements
        .map(|element| match element {
            ArrayElement::Value(value) => const_string(value.value, file.content, file.scope),
            _ => None,
        })
        .collect()
}
