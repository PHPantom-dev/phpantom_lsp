use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;

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
    /// Container binding key → concrete class FQN, for the bindings a
    /// provider makes under a *string* abstract
    /// (`$this->app->singleton('sentry', fn () => new HubAdapter())`).  A
    /// binding keyed by `Contract::class` needs no table: the written name
    /// already resolves.
    pub bindings: HashMap<String, String>,
    /// A provider rebound `translator` or `translation.loader` to something
    /// other than Laravel's own file-based pair, so the strings come from a
    /// source we cannot enumerate (a database table, say) and the set of
    /// valid translation keys is unknowable.
    pub custom_translation_loader: bool,
}

impl ProviderResources {
    pub fn merge(&mut self, other: ProviderResources) {
        self.config_files.extend(other.config_files);
        self.view_dirs.extend(other.view_dirs);
        self.trans_dirs.extend(other.trans_dirs);
        self.route_files.extend(other.route_files);
        self.bindings.extend(other.bindings);
        self.custom_translation_loader |= other.custom_translation_loader;
    }
}

/// Container keys whose binding decides where translation strings come from.
const TRANSLATION_BINDINGS: [&str; 2] = ["translator", "translation.loader"];

/// The classes Laravel's own `TranslationServiceProvider` binds those keys
/// to.  A factory that builds anything else reads its lines from somewhere
/// other than the `lang/` directories we scan.
const FILE_TRANSLATION_CLASSES: [&str; 2] = ["FileLoader", "Translator"];

/// Container methods that put a new value behind a key.
const BINDING_METHODS: &[&[u8]] = &[
    b"bind",
    b"bindif",
    b"singleton",
    b"singletonif",
    b"scoped",
    b"scopedif",
    b"instance",
    b"extend",
];

pub(crate) fn extract_provider_resources(
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
) -> ProviderResources {
    let mut resources = ProviderResources::default();
    let file_dir = file_path.parent().unwrap_or(file_path);
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
    // Container bindings name their concrete by short name (`new HubAdapter()`
    // under a `use` statement), so the file's resolved-name table is needed to
    // turn that into the FQN the class index is keyed by.
    let resolved = OwnedResolvedNames::from_resolved(&NameResolver::new(&arena).resolve(program));

    super::helpers::walk_program_expressions(program, &mut |expr| {
        // Any direct use of the `Route` facade means routes are registered
        // from this file rather than only pointed at.
        if let Expression::Call(Call::StaticMethod(sc)) = expr
            && let Expression::Identifier(id) = sc.class
            && id
                .value()
                .rsplit(|&b| b == b'\\')
                .next()
                .is_some_and(|seg| seg.eq_ignore_ascii_case(b"Route"))
        {
            registers_routes_inline = true;
        }

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
            && let Some(path) = resolve_path_arg(
                first_arg.value(),
                content,
                file_dir,
                workspace_root,
                program,
            )
        {
            grouped_route_files.push(path);
            return ControlFlow::Continue(());
        }

        // `$this->app->singleton('translation.loader', …)` and friends decide
        // where translation lines come from, and the container is reached
        // through `$this->app`, not `$this`, so this is checked ahead of the
        // `$this->…` resource loaders below.
        if BINDING_METHODS.contains(&method_lower.as_slice())
            && is_app_container_expr(mc.object)
            && let Some(key_arg) = mc.argument_list.arguments.iter().next()
            && let Some((key, _, _)) =
                super::helpers::extract_string_literal(key_arg.value(), content)
        {
            let factory = mc.argument_list.arguments.iter().nth(1).map(|a| a.value());

            if TRANSLATION_BINDINGS.contains(&key) {
                // `extend` decorates whatever is already bound, so even a
                // file-based wrapper adds lines from somewhere else.
                if method_lower != b"extend" && builds_file_translator(factory) {
                    return ControlFlow::Continue(());
                }
                resources.custom_translation_loader = true;
                return ControlFlow::Continue(());
            }

            // `extend` wraps whatever the key already holds; which class comes
            // out depends on the binding it decorates, so only the calls that
            // *replace* the value tell us the concrete type.
            if method_lower != b"extend"
                && let Some(concrete) = binding_concrete(factory, &resolved)
            {
                resources.bindings.insert(key.to_string(), concrete);
            }
            return ControlFlow::Continue(());
        }

        if !is_this_expr(mc.object) {
            return ControlFlow::Continue(());
        }

        let args: Vec<_> = mc.argument_list.arguments.iter().collect();

        if method_lower == b"mergeconfigfrom" && args.len() >= 2 {
            if let Some(path) =
                resolve_path_arg(args[0].value(), content, file_dir, workspace_root, program)
                && let Some((ns, _, _)) =
                    super::helpers::extract_string_literal(args[1].value(), content)
            {
                resources.config_files.push(ProviderResource {
                    path,
                    namespace: ns.to_string(),
                });
            }
        } else if method_lower == b"loadviewsfrom" && args.len() >= 2 {
            if let Some(path) =
                resolve_path_arg(args[0].value(), content, file_dir, workspace_root, program)
                && let Some((ns, _, _)) =
                    super::helpers::extract_string_literal(args[1].value(), content)
            {
                resources.view_dirs.push(ProviderResource {
                    path,
                    namespace: ns.to_string(),
                });
            }
        } else if method_lower == b"loadtranslationsfrom" && args.len() >= 2 {
            if let Some(path) =
                resolve_path_arg(args[0].value(), content, file_dir, workspace_root, program)
                && let Some((ns, _, _)) =
                    super::helpers::extract_string_literal(args[1].value(), content)
            {
                resources.trans_dirs.push(ProviderResource {
                    path,
                    namespace: ns.to_string(),
                });
            }
        } else if method_lower == b"loadjsontranslationsfrom" && !args.is_empty() {
            if let Some(path) =
                resolve_path_arg(args[0].value(), content, file_dir, workspace_root, program)
            {
                resources.trans_dirs.push(ProviderResource {
                    path,
                    namespace: String::new(),
                });
            }
        } else if method_lower == b"loadroutesfrom"
            && !args.is_empty()
            && let Some(path) =
                resolve_path_arg(args[0].value(), content, file_dir, workspace_root, program)
        {
            resources.route_files.push(path);
        }

        ControlFlow::Continue(())
    });

    if registers_routes_inline {
        resources.route_files.push(file_path.to_path_buf());
    } else {
        resources.route_files.extend(grouped_route_files);
    }

    resources
}

fn is_this_expr(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
    )
}

/// Whether `expr` names the service container: `$this->app` in a provider
/// method, the `$app` a deferred callback receives, or the `app()` helper.
fn is_app_container_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => dv.name == b"$app",
        Expression::Access(Access::Property(pa)) => {
            is_this_expr(pa.object)
                && matches!(
                    &pa.property,
                    ClassLikeMemberSelector::Identifier(ident)
                        if ident.value.eq_ignore_ascii_case(b"app")
                )
        }
        Expression::Call(Call::Function(fc)) => {
            matches!(fc.function, Expression::Identifier(id)
                if id.value()
                    .rsplit(|&b| b == b'\\')
                    .next()
                    .is_some_and(|seg| seg.eq_ignore_ascii_case(b"app")))
                && fc.argument_list.arguments.is_empty()
        }
        _ => false,
    }
}

/// The concrete class a container binding puts behind its key.
///
/// Covers the shapes a service provider writes: the class itself
/// (`bind('foo', Foo::class)`), a ready-made instance
/// (`instance('foo', new Foo())`), and the usual factory
/// (`singleton('foo', fn () => new Foo())` or its closure equivalent, whose
/// first `return` decides the type).  A factory that hands back anything else
/// (a container lookup, a variable, a conditional) yields `None`: guessing
/// there would bind the key to a class the application never resolves.
fn binding_concrete(
    expr: Option<&Expression<'_>>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    match expr? {
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(id) => resolved_class_fqn(id, resolved),
            _ => None,
        },
        Expression::Access(Access::ClassConstant(cca))
            if matches!(
                &cca.constant,
                ClassLikeConstantSelector::Identifier(constant)
                    if constant.value.eq_ignore_ascii_case(b"class")
            ) =>
        {
            match cca.class {
                Expression::Identifier(id) => resolved_class_fqn(id, resolved),
                _ => None,
            }
        }
        Expression::ArrowFunction(arrow) => binding_concrete(Some(arrow.expression), resolved),
        Expression::Closure(closure) => {
            closure.body.statements.iter().find_map(|stmt| match stmt {
                Statement::Return(ret) => binding_concrete(ret.value, resolved),
                _ => None,
            })
        }
        Expression::Parenthesized(inner) => binding_concrete(Some(inner.expression), resolved),
        _ => None,
    }
}

/// The FQN a class-name identifier resolves to, through the file's namespace
/// and `use` statements, falling back to the written name when the resolver
/// did not track the offset.
fn resolved_class_fqn(ident: &Identifier<'_>, resolved: &OwnedResolvedNames) -> Option<String> {
    if let Some(fqn) = resolved.get(ident.span().start.offset) {
        return Some(fqn.trim_start_matches('\\').to_string());
    }
    let raw = bytes_to_str(ident.value()).trim_start_matches('\\');
    (!raw.is_empty()).then(|| raw.to_string())
}

/// Whether a translation binding's factory builds Laravel's own file-based
/// translator, i.e. every class it names is one of `FILE_TRANSLATION_CLASSES`.
///
/// A factory that reaches for anything else has moved the lines out of the
/// `lang/` directories, and one that names no class at all (a container
/// lookup, a variable) says nothing either way, which is equally unknowable.
fn builds_file_translator(factory: Option<&Expression<'_>>) -> bool {
    let Some(factory) = factory else {
        return false;
    };

    let mut named_any = false;
    let mut all_file_based = true;
    super::helpers::walk_expression_tree(factory, &mut |expr| {
        if let Some(name) = instantiated_or_class_string(expr) {
            named_any = true;
            if !FILE_TRANSLATION_CLASSES
                .iter()
                .any(|known| crate::util::short_name(name).eq_ignore_ascii_case(known))
            {
                all_file_based = false;
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    });

    named_any && all_file_based
}

/// The class an expression names, either by instantiating it (`new X(…)`) or
/// by referring to it as a string (`X::class`).
///
/// An empty name means the expression names a class that is only known at
/// runtime (`new $loaderClass`), which no more resolves to Laravel's own
/// loader than an explicit replacement does.
fn instantiated_or_class_string<'arena>(expr: &Expression<'arena>) -> Option<&'arena str> {
    let class = match expr {
        Expression::Instantiation(inst) => inst.class,
        Expression::Access(Access::ClassConstant(access))
            if matches!(
                &access.constant,
                ClassLikeConstantSelector::Identifier(constant)
                    if constant.value.eq_ignore_ascii_case(b"class")
            ) =>
        {
            access.class
        }
        _ => return None,
    };
    match class {
        Expression::Identifier(id) => Some(crate::atom::bytes_to_str(id.value())),
        _ => Some(""),
    }
}

/// Resolve an expression that names a file to the path it points at.
///
/// Covers the forms Laravel projects use to locate route, config, view, and
/// translation files: `__DIR__ . '/…'`, `base_path('…')`, a bare literal
/// (absolute, or relative to the referring file), and a local variable
/// assigned one of those forms earlier in the same scope (Livewire's
/// service provider writes `$config = __DIR__.'/../config/x.php';` before
/// passing `$config` to `mergeConfigFrom`).  `program` is the parse of
/// `content`, which that last form is resolved against.
pub(crate) fn resolve_path_arg(
    expr: &Expression<'_>,
    content: &str,
    file_dir: &Path,
    workspace_root: &Path,
    program: &Program<'_>,
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

    if let Expression::Variable(Variable::Direct(dv)) = expr {
        let assigned = last_assignment_before(program, dv.start_offset(), dv.name)?;
        return resolve_path_arg(assigned, content, file_dir, workspace_root, program);
    }

    None
}

/// The RHS of the last `$name = <expr>;` assignment before `offset` in the
/// scope enclosing it: PHP's own resolution rule for a variable read, the
/// most recent write to it in the same scope.
///
/// A service provider assigns inside a method; a route file assigns at the
/// top level of the script, where the enclosing scope is the file itself.
fn last_assignment_before<'ast, 'arena>(
    program: &'ast Program<'arena>,
    offset: u32,
    name: &[u8],
) -> Option<&'ast Expression<'arena>> {
    let mut best: Option<(u32, &'ast Expression<'arena>)> = None;
    let mut record = |node: Node<'ast, 'arena>| {
        let Node::Assignment(assignment) = node else {
            return;
        };
        if !assignment.operator.is_assign() {
            return;
        }
        let Expression::Variable(Variable::Direct(target)) = assignment.lhs else {
            return;
        };
        if target.name != name {
            return;
        }
        let end = node.span().end.offset;
        if super::helpers::beats_best(&best, end, offset) {
            best = Some((end, assignment.rhs));
        }
    };

    match super::helpers::enclosing_body(Node::Program(program), offset) {
        Some(body) => super::helpers::walk_before_cursor(body, offset, &mut record),
        None => super::helpers::walk_file_scope_before_cursor(
            Node::Program(program),
            offset,
            &mut record,
        ),
    }
    best.map(|(_, rhs)| rhs)
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
        // `$this->loadRoutesFrom(...)`.  Because the provider touches the
        // `Route` facade it is itself the route source: scanning it applies
        // the group's prefixes to the file it points at.
        let content = "<?php\n\
            class RouteServiceProvider {\n\
                protected function mapWebRoutes(): void {\n\
                    Route::middleware('web')\n\
                        ->namespace($this->namespace)\n\
                        ->group(base_path('app/Contexts/Backoffice/Routes/web.php'));\n\
                }\n\
            }\n";
        let file_path = Path::new("/ws/app/Providers/RouteServiceProvider.php");
        let resources = extract_provider_resources(content, file_path, Path::new("/ws"));
        assert_eq!(
            resources.route_files,
            vec![file_path.to_path_buf()],
            "a provider that uses the Route facade is scanned as a route source"
        );
    }

    #[test]
    fn treats_inline_route_registration_as_a_route_source() {
        // An inline `Route::group(function () { ... })` registers its routes
        // in the provider itself, so the provider is the file to scan.
        let content = "<?php\n\
            Route::middleware('web')->group(function () {\n\
                Route::get('/')->name('home');\n\
            });\n";
        let file_path = Path::new("/ws/app/Providers/RouteServiceProvider.php");
        let resources = extract_provider_resources(content, file_path, Path::new("/ws"));
        assert_eq!(resources.route_files, vec![file_path.to_path_buf()]);
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
        let file_path = Path::new("/ws/vendor/acme/src/PackageServiceProvider.php");
        let resources = extract_provider_resources(content, file_path, Path::new("/ws"));
        assert_eq!(
            resources.route_files,
            vec![Path::new("/ws/vendor/acme/src").join("../routes/pkg.php")],
            "loadRoutesFrom must still be detected"
        );
    }

    #[test]
    fn ignores_non_route_facade_group() {
        // A `->group()` call whose chain does not root at the Route facade
        // must not be misread as a route-file registration.
        let content = "<?php\n\
            Blade::directive('x')->group(base_path('resources/views'));\n";
        let resources =
            extract_provider_resources(content, Path::new("/ws/Provider.php"), Path::new("/ws"));
        assert!(resources.route_files.is_empty());
    }

    #[test]
    fn resolves_config_path_behind_a_local_variable() {
        // Livewire's own service provider assigns the path to a local
        // variable before passing it to `mergeConfigFrom`, rather than
        // writing the `__DIR__ . '...'` concatenation inline.
        let content = "<?php\n\
            class LivewireServiceProvider {\n\
                protected function registerConfig(): void {\n\
                    $config = __DIR__.'/../config/livewire.php';\n\
                    $this->mergeConfigFrom($config, 'livewire');\n\
                }\n\
            }\n";
        let file_path = Path::new("/ws/vendor/livewire/livewire/src/LivewireServiceProvider.php");
        let resources = extract_provider_resources(content, file_path, Path::new("/ws"));
        assert_eq!(resources.config_files.len(), 1);
        assert_eq!(
            resources.config_files[0].path,
            Path::new("/ws/vendor/livewire/livewire/src").join("../config/livewire.php")
        );
        assert_eq!(resources.config_files[0].namespace, "livewire");
    }

    #[test]
    fn detects_a_database_backed_translation_loader() {
        // An application that keeps its strings in a database still builds a
        // FileLoader to hand to its own loader, so the decision has to follow
        // what the factory *returns*, not merely which classes it mentions.
        let content = "<?php\n\
            class TranslationServiceProvider {\n\
                public function register(): void {\n\
                    $this->app->singleton('translation.loader', function ($app) {\n\
                        $fileLoader = new FileLoader($app->make('files'), $app->make('path.lang'));\n\
                        return new DatabaseTranslationLoader($fileLoader);\n\
                    });\n\
                }\n\
            }\n";
        let resources = extract_provider_resources(
            content,
            Path::new("/ws/src/TranslationServiceProvider.php"),
            Path::new("/ws"),
        );
        assert!(resources.custom_translation_loader);
    }

    #[test]
    fn detects_a_replaced_translator() {
        let content = "<?php\n\
            class TranslationServiceProvider {\n\
                public function register(): void {\n\
                    $this->app->singleton('translator', fn ($app) => new DatabaseTranslator(\n\
                        $app->make('translation.loader'),\n\
                        $app->getLocale(),\n\
                    ));\n\
                }\n\
            }\n";
        let resources = extract_provider_resources(
            content,
            Path::new("/ws/src/TranslationServiceProvider.php"),
            Path::new("/ws"),
        );
        assert!(resources.custom_translation_loader);
    }

    #[test]
    fn laravels_own_translation_bindings_are_not_a_replacement() {
        // Laravel's own TranslationServiceProvider is itself scanned when the
        // project lists the framework providers in `config/app.php`.  Reading
        // its bindings as a replacement would silence translation diagnostics
        // for every Laravel project.
        let content = "<?php\n\
            class TranslationServiceProvider {\n\
                public function register(): void {\n\
                    $this->app->singleton('translator', function ($app) {\n\
                        $loader = $app['translation.loader'];\n\
                        $trans = new Translator($loader, $app->getLocale());\n\
                        $trans->setFallback($app->getFallbackLocale());\n\
                        return $trans;\n\
                    });\n\
                    $this->registerLoader();\n\
                }\n\
                protected function registerLoader(): void {\n\
                    $this->app->singleton('translation.loader', function ($app) {\n\
                        return new FileLoader($app['files'], [__DIR__.'/lang', $app['path.lang']]);\n\
                    });\n\
                }\n\
            }\n";
        let resources = extract_provider_resources(
            content,
            Path::new(
                "/ws/vendor/laravel/framework/src/Illuminate/Translation/TranslationServiceProvider.php",
            ),
            Path::new("/ws"),
        );
        assert!(!resources.custom_translation_loader);
    }

    #[test]
    fn a_decorated_translation_loader_counts_as_a_replacement() {
        // `extend` wraps whatever is already bound, so the lines it serves are
        // not limited to the ones on disk even when the wrapper is file-based.
        let content = "<?php\n\
            class CacheTranslationServiceProvider {\n\
                public function register(): void {\n\
                    $this->app->extend('translation.loader', fn ($loader) => new FileLoader($loader));\n\
                }\n\
            }\n";
        let resources = extract_provider_resources(
            content,
            Path::new("/ws/src/CacheTranslationServiceProvider.php"),
            Path::new("/ws"),
        );
        assert!(resources.custom_translation_loader);
    }

    #[test]
    fn unrelated_container_bindings_are_ignored() {
        let content = "<?php\n\
            class AppServiceProvider {\n\
                public function register(): void {\n\
                    $this->app->singleton('sentry', fn () => new HubAdapter());\n\
                    $this->app->bind(Contract::class, Implementation::class);\n\
                }\n\
            }\n";
        let resources = extract_provider_resources(
            content,
            Path::new("/ws/src/AppServiceProvider.php"),
            Path::new("/ws"),
        );
        assert!(!resources.custom_translation_loader);
    }

    #[test]
    fn local_variable_scan_stays_within_its_own_method() {
        // `$path` is assigned in `registerConfig` but `registerViews` never
        // assigns it: resolving `registerViews`'s `$path` must not pick up
        // the other method's assignment.
        let content = "<?php\n\
            class PackageServiceProvider {\n\
                public function registerConfig(): void {\n\
                    $path = __DIR__.'/../config/a.php';\n\
                    $this->mergeConfigFrom($path, 'a');\n\
                }\n\
                public function registerViews(): void {\n\
                    $this->loadViewsFrom($path, 'b');\n\
                }\n\
            }\n";
        let file_path = Path::new("/ws/vendor/acme/src/PackageServiceProvider.php");
        let resources = extract_provider_resources(content, file_path, Path::new("/ws"));
        assert_eq!(resources.config_files.len(), 1);
        assert!(
            resources.view_dirs.is_empty(),
            "an undefined `$path` in a different method must not resolve to another method's assignment"
        );
    }
}
