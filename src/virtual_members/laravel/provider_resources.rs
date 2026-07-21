use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use mago_syntax::cst::*;

#[derive(Debug, Clone)]
pub(crate) struct ProviderResource {
    pub path: PathBuf,
    pub namespace: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProviderResources {
    pub config_files: Vec<ProviderResource>,
    pub view_dirs: Vec<ProviderResource>,
    pub trans_dirs: Vec<ProviderResource>,
    pub route_files: Vec<PathBuf>,
}

impl ProviderResources {
    pub fn merge(&mut self, other: ProviderResources) {
        self.config_files.extend(other.config_files);
        self.view_dirs.extend(other.view_dirs);
        self.trans_dirs.extend(other.trans_dirs);
        self.route_files.extend(other.route_files);
    }
}

pub(crate) fn extract_provider_resources(
    content: &str,
    file_dir: &Path,
    workspace_root: &Path,
) -> ProviderResources {
    let mut resources = ProviderResources::default();

    super::helpers::walk_all_php_expressions(content, &mut |expr| {
        let Expression::Call(Call::Method(mc)) = expr else {
            return ControlFlow::Continue(());
        };

        let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
            return ControlFlow::Continue(());
        };

        let method_lower = ident.value.to_ascii_lowercase();

        // `Route::middleware(...)->group(base_path('routes/web.php'))` registers
        // a route file without `$this->loadRoutesFrom(...)`.  The `->group()`
        // argument is either a closure (inline routes, ignored here) or a path
        // to a file whose routes we must scan.
        if method_lower == b"group"
            && chain_roots_at_route(mc.object)
            && let Some(first_arg) = mc.argument_list.arguments.iter().next()
            && let Some(path) =
                resolve_path_arg(first_arg.value(), content, file_dir, workspace_root)
        {
            resources.route_files.push(path);
            return ControlFlow::Continue(());
        }

        if !is_this_expr(mc.object) {
            return ControlFlow::Continue(());
        }

        let args: Vec<_> = mc.argument_list.arguments.iter().collect();

        if method_lower == b"mergeconfigfrom" && args.len() >= 2 {
            if let Some(path) = resolve_path_arg(args[0].value(), content, file_dir, workspace_root)
                && let Some((ns, _, _)) =
                    super::helpers::extract_string_literal(args[1].value(), content)
            {
                resources.config_files.push(ProviderResource {
                    path,
                    namespace: ns.to_string(),
                });
            }
        } else if method_lower == b"loadviewsfrom" && args.len() >= 2 {
            if let Some(path) = resolve_path_arg(args[0].value(), content, file_dir, workspace_root)
                && let Some((ns, _, _)) =
                    super::helpers::extract_string_literal(args[1].value(), content)
            {
                resources.view_dirs.push(ProviderResource {
                    path,
                    namespace: ns.to_string(),
                });
            }
        } else if method_lower == b"loadtranslationsfrom" && args.len() >= 2 {
            if let Some(path) = resolve_path_arg(args[0].value(), content, file_dir, workspace_root)
                && let Some((ns, _, _)) =
                    super::helpers::extract_string_literal(args[1].value(), content)
            {
                resources.trans_dirs.push(ProviderResource {
                    path,
                    namespace: ns.to_string(),
                });
            }
        } else if method_lower == b"loadjsontranslationsfrom" && !args.is_empty() {
            if let Some(path) = resolve_path_arg(args[0].value(), content, file_dir, workspace_root)
            {
                resources.trans_dirs.push(ProviderResource {
                    path,
                    namespace: String::new(),
                });
            }
        } else if method_lower == b"loadroutesfrom"
            && !args.is_empty()
            && let Some(path) = resolve_path_arg(args[0].value(), content, file_dir, workspace_root)
        {
            resources.route_files.push(path);
        }

        ControlFlow::Continue(())
    });

    resources
}

fn is_this_expr(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
    )
}

fn resolve_path_arg(
    expr: &Expression<'_>,
    content: &str,
    file_dir: &Path,
    workspace_root: &Path,
) -> Option<PathBuf> {
    if let Some(rel) = super::helpers::extract_dir_concat_path(expr, content) {
        let resolved = file_dir.join(rel.trim_start_matches('/'));
        return resolved.canonicalize().ok().or(Some(resolved));
    }

    // `base_path('app/.../web.php')` resolves relative to the workspace root.
    if let Expression::Call(Call::Function(fc)) = expr
        && let Expression::Identifier(id) = fc.function
        && id
            .value()
            .rsplit(|&b| b == b'\\')
            .next()
            .is_some_and(|seg| seg.eq_ignore_ascii_case(b"base_path"))
        && let Some(first_arg) = fc.argument_list.arguments.iter().next()
        && let Some((val, _, _)) =
            super::helpers::extract_string_literal(first_arg.value(), content)
    {
        let resolved = workspace_root.join(val.trim_start_matches('/'));
        return resolved.canonicalize().ok().or(Some(resolved));
    }

    if let Some((val, _, _)) = super::helpers::extract_string_literal(expr, content) {
        if val.starts_with('/') {
            let p = PathBuf::from(val);
            return p.canonicalize().ok().or(Some(p));
        }
        let resolved = file_dir.join(val);
        return resolved.canonicalize().ok().or(Some(resolved));
    }

    None
}

/// Check whether an instance-method call chain roots at the `Route` facade,
/// i.e. `Route::middleware(...)->namespace(...)->…`.  Walks down the `->object`
/// chain until it reaches the static entry point and matches its class name.
fn chain_roots_at_route(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Call(Call::Method(mc)) => chain_roots_at_route(mc.object),
        Expression::Call(Call::StaticMethod(sc)) => {
            if let Expression::Identifier(id) = sc.class {
                id.value()
                    .rsplit(|&b| b == b'\\')
                    .next()
                    .is_some_and(|seg| seg.eq_ignore_ascii_case(b"Route"))
            } else {
                false
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_route_group_base_path_registration() {
        // A RouteServiceProvider that registers routes via the fluent
        // `Route::middleware(...)->group(base_path('...'))` API rather than
        // `$this->loadRoutesFrom(...)`.
        let content = "<?php\n\
            class RouteServiceProvider {\n\
                protected function mapWebRoutes(): void {\n\
                    Route::middleware('web')\n\
                        ->namespace($this->namespace)\n\
                        ->group(base_path('app/Contexts/Backoffice/Routes/web.php'));\n\
                }\n\
            }\n";
        let file_dir = Path::new("/ws/app/Providers");
        let root = Path::new("/ws");
        let resources = extract_provider_resources(content, file_dir, root);
        assert_eq!(
            resources.route_files,
            vec![root.join("app/Contexts/Backoffice/Routes/web.php")],
            "Route::...->group(base_path(...)) should register the route file"
        );
    }

    #[test]
    fn ignores_route_group_with_closure_body() {
        // An inline `Route::group(function () { ... })` has no file to scan.
        let content = "<?php\n\
            Route::middleware('web')->group(function () {\n\
                Route::get('/')->name('home');\n\
            });\n";
        let resources =
            extract_provider_resources(content, Path::new("/ws/routes"), Path::new("/ws"));
        assert!(
            resources.route_files.is_empty(),
            "closure group bodies are not route files"
        );
    }

    #[test]
    fn still_detects_load_routes_from() {
        // The existing `$this->loadRoutesFrom(__DIR__ . '/routes.php')` path
        // must keep working alongside the new fluent detection.
        let content = "<?php\n\
            class PackageServiceProvider {\n\
                public function boot(): void {\n\
                    $this->loadRoutesFrom(__DIR__ . '/../routes/pkg.php');\n\
                }\n\
            }\n";
        let file_dir = Path::new("/ws/vendor/acme/src");
        let resources = extract_provider_resources(content, file_dir, Path::new("/ws"));
        assert_eq!(
            resources.route_files,
            vec![file_dir.join("../routes/pkg.php")],
            "loadRoutesFrom must still be detected"
        );
    }

    #[test]
    fn ignores_non_route_facade_group() {
        // A `->group()` call whose chain does not root at the Route facade
        // must not be misread as a route-file registration.
        let content = "<?php\n\
            Blade::directive('x')->group(base_path('resources/views'));\n";
        let resources = extract_provider_resources(content, Path::new("/ws"), Path::new("/ws"));
        assert!(resources.route_files.is_empty());
    }
}
