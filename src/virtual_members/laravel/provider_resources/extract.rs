//! Scanning one service provider file for the resources it registers.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::*;

use super::super::const_eval::{ClassContext, Scope};
use super::super::expr_walker::walk_program_expressions;
use super::super::helpers::extract_string_literal;
use super::bindings::{record_container_call, scan_property_bindings};
use super::package_tools::record_package_call;
use super::views::{
    push_view_dir_registration, record_blade_compiler_call, record_blade_facade_call,
    record_view_data, record_view_namespace,
};
use super::{PathContext, ProviderIdentity, ProviderResource, ProviderResources, resolve_path_arg};
use crate::names::OwnedResolvedNames;
use crate::symbol_map::extraction::laravel::{chain_roots_at_facade, config_keys_written_by};

/// How many `->` links a `Route::…->group(path)` registration may put
/// between the facade and the `group()` call. `Route::middleware(…)
/// ->prefix(…)->name(…)->group(…)` is already longer than a provider
/// realistically writes, and the bound keeps the walk linear.
const ROUTE_GROUP_CHAIN_DEPTH: usize = 8;

/// The provider file a scan is reading, and what its registrations are read
/// against.
pub(super) struct ProviderFile<'a, 'arena> {
    pub(super) content: &'a str,
    pub(super) path: &'a Path,
    /// The provider class's own constants and static property defaults,
    /// merged over its parent chain, which a key written as
    /// `static::$abstract` folds through.
    pub(super) scope: &'a Scope,
    /// The file's resolved names.  Container bindings name their concrete
    /// by short name (`new HubAdapter()` under a `use` statement), and this
    /// is what turns that into the FQN the class index is keyed by.
    pub(super) resolved: &'a OwnedResolvedNames,
    pub(super) paths: PathContext<'a, 'arena>,
    /// Which provider this is, so a key two providers bind ends up with
    /// the class the container would hold.
    pub(super) provider: &'a Arc<ProviderIdentity>,
}

/// Scan a service provider for the resources it registers.
///
/// `class_context` carries the provider class's own constants and static
/// property defaults, merged over its parent chain, so a binding key written
/// as `static::$abstract` folds to the string it holds.  `provider` records
/// which provider this is, so a key two providers bind ends up with the class
/// the container would hold.
pub(crate) fn extract_provider_resources(
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    class_context: ClassContext,
    provider: Arc<ProviderIdentity>,
) -> ProviderResources {
    let mut resources = ProviderResources::default();
    let scope = Scope::for_class(class_context);
    // Route files reached through `Route::…->group('path')`.  They are only
    // kept when the provider turns out not to register any routes inline:
    // an inline registration means the provider itself is scanned as a route
    // source, and that scan reaches the same files *with* the name and URI
    // prefixes their enclosing group applies.
    let mut grouped_route_files: Vec<PathBuf> = Vec::new();
    let mut registers_routes_inline = false;

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = OwnedResolvedNames::from_resolved(&NameResolver::new(&arena).resolve(program));
    let file = ProviderFile {
        content,
        path: file_path,
        scope: &scope,
        resolved: &resolved,
        paths: PathContext {
            file_dir: file_path.parent().unwrap_or(file_path),
            workspace_root,
            program,
        },
        provider: &provider,
    };

    walk_program_expressions(program, &mut |expr| {
        resources
            .config_writes
            .extend(config_keys_written_by(expr, content));

        // Any direct use of the `Route` facade means routes are registered
        // from this file rather than only pointed at.
        if let Expression::Call(Call::StaticMethod(call)) = expr
            && names_facade(call.class, b"Route")
        {
            registers_routes_inline = true;
        }

        // The view registrations are also written against the container's
        // view factory, which reaches them as method calls, so they are
        // tried ahead of the method-call registrations below.
        if record_blade_facade_call(&file, expr, &mut resources)
            || record_view_namespace(&file, expr, &mut resources)
            || record_view_data(&file, expr, &mut resources)
        {
            return ControlFlow::Continue(());
        }

        let Expression::Call(Call::Method(call)) = expr else {
            return ControlFlow::Continue(());
        };
        let ClassLikeMemberSelector::Identifier(ident) = &call.method else {
            return ControlFlow::Continue(());
        };
        let method = ident.value.to_ascii_lowercase();

        // `Route::middleware(...)->group(base_path('routes/web.php'))` registers
        // a route file without `$this->loadRoutesFrom(...)`.  The `->group()`
        // argument is either a closure (inline routes, ignored here) or a path
        // to a file whose routes we must scan.
        if method == b"group"
            && chain_roots_at_facade(call.object, ROUTE_GROUP_CHAIN_DEPTH, &|name| {
                name.eq_ignore_ascii_case("Route")
            })
            && let Some(first_arg) = call.argument_list.arguments.iter().next()
            && let Some(path) = resolve_path_arg(first_arg.value(), content, &file.paths)
        {
            grouped_route_files.push(path);
            return ControlFlow::Continue(());
        }

        if record_container_call(&file, &method, call, &mut resources)
            || record_blade_compiler_call(&file, &method, call, &mut resources)
            || record_package_call(&file, &method, call, &mut resources)
        {
            return ControlFlow::Continue(());
        }
        record_resource_loader(&file, &method, call, &mut resources);
        ControlFlow::Continue(())
    });

    // `public $bindings = ['payments' => StripeGateway::class]` and its
    // `$singletons` twin are applied by `Application::register()` *after* the
    // provider's own `register()` has run, so they overwrite what the walk
    // above found for the same key, exactly as the container would.
    for stmt in program.statements.iter() {
        scan_property_bindings(&file, stmt, &mut resources);
    }

    if registers_routes_inline {
        resources.route_files.push(file_path.to_path_buf());
    } else {
        resources.route_files.extend(grouped_route_files);
    }

    resources.folio_mounts =
        super::super::folio::scan_folio_mounts_in_program(content, &file.paths);

    resources
}

/// Whether `class` names the facade `name` (`Route`, `Blade`), written
/// bare, imported, or fully qualified.
pub(super) fn names_facade(class: &Expression<'_>, name: &[u8]) -> bool {
    let Expression::Identifier(id) = class else {
        return false;
    };
    id.value()
        .rsplit(|&b| b == b'\\')
        .next()
        .is_some_and(|segment| segment.eq_ignore_ascii_case(name))
}

/// Record a resource registered through one of `ServiceProvider`'s own
/// loaders (`$this->mergeConfigFrom(…)`, `$this->loadViewsFrom(…)`, …).
fn record_resource_loader(
    file: &ProviderFile<'_, '_>,
    method: &[u8],
    call: &MethodCall<'_>,
    resources: &mut ProviderResources,
) {
    if !is_this_expr(call.object) {
        return;
    }

    let content = file.content;
    let args: Vec<_> = call.argument_list.arguments.iter().collect();

    // Config registrations require a key, while translations can omit
    // their namespace to add a global path. Views also check for a
    // published-package override, so they are handled separately below.
    let registrations: Option<&mut Vec<ProviderResource>> = match method {
        b"mergeconfigfrom" => Some(&mut resources.config_files),
        b"loadtranslationsfrom" => Some(&mut resources.trans_dirs),
        _ => None,
    };
    if let Some(target) = registrations {
        let namespace = match args.get(1).map(|arg| arg.value()) {
            None | Some(Expression::Literal(Literal::Null(_)))
                if method == b"loadtranslationsfrom" =>
            {
                Some("")
            }
            Some(arg) => extract_string_literal(arg, content).map(|(namespace, _, _)| namespace),
            None => None,
        };
        let path = args
            .first()
            .and_then(|arg| resolve_path_arg(arg.value(), content, &file.paths));
        match (namespace, path) {
            (Some(namespace), Some(path)) => target.push(ProviderResource {
                path,
                namespace: namespace.to_string(),
            }),
            // The diagnostic that judges config keys needs to know that a
            // namespace exists even when its file cannot be enumerated.
            (Some(namespace), None) if method == b"mergeconfigfrom" && !args.is_empty() => {
                resources
                    .unread_config_namespaces
                    .push(namespace.to_string());
            }
            (None, _)
                if method == b"mergeconfigfrom"
                    && args.get(1).is_some_and(|arg| {
                        !matches!(arg.value(), Expression::Literal(Literal::Null(_)))
                    }) =>
            {
                resources.open_config_namespace = true;
            }
            _ => {}
        }
    } else if method == b"loadviewsfrom" {
        if args.len() >= 2
            && let Some(path) = resolve_path_arg(args[0].value(), content, &file.paths)
            && let Some((ns, _, _)) = extract_string_literal(args[1].value(), content)
        {
            push_view_dir_registration(resources, path, ns.to_string(), file.paths.workspace_root);
        }
    } else if method == b"loadjsontranslationsfrom" && !args.is_empty() {
        if let Some(path) = resolve_path_arg(args[0].value(), content, &file.paths) {
            resources.trans_dirs.push(ProviderResource {
                path,
                namespace: String::new(),
            });
        }
    } else if method == b"loadroutesfrom"
        && !args.is_empty()
        && let Some(path) = resolve_path_arg(args[0].value(), content, &file.paths)
    {
        resources.route_files.push(path);
    }
}

fn is_this_expr(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
    )
}
