use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::const_eval::ClassContext;
use super::*;

#[test]
fn records_registered_class_component_namespaces() {
    // Both shapes a package registers its components with: the facade,
    // and the compiler instance a deferred callback receives.  A
    // single-quoted namespace keeps its doubled separators in source,
    // so the recorded value must be the namespace itself.
    let content = "<?php\n\
        class PackageServiceProvider {\n\
            public function boot(): void {\n\
                Blade::componentNamespace('Nightshade\\\\Views\\\\Components', 'nightshade');\n\
                $this->callAfterResolving('blade.compiler', function ($blade) {\n\
                    $blade->componentNamespace('Acme\\\\Ui', 'acme');\n\
                });\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/app/Providers/PackageServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(
        resources.class_component_namespaces,
        vec![
            (
                "nightshade".to_string(),
                "Nightshade\\Views\\Components".to_string()
            ),
            ("acme".to_string(), "Acme\\Ui".to_string()),
        ]
    );
}

#[test]
fn records_registered_anonymous_component_directories() {
    // The directory is normalised to the dot notation a view name is
    // written in, and a registration with no prefix of its own is
    // addressed under the directory it names.
    let content = "<?php\n\
        class AppServiceProvider {\n\
            public function boot(): void {\n\
                Blade::anonymousComponentNamespace('components', 'webshop');\n\
                Blade::anonymousComponentNamespace('theme/components');\n\
                Blade::anonymousComponentPath(resource_path('views/ui'), 'ui');\n\
                Blade::anonymousComponentPath(__DIR__ . '/../views/pkg');\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/app/Providers/AppServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(
        resources.anonymous_component_namespaces,
        vec![
            ("webshop".to_string(), "components".to_string()),
            (
                "theme/components".to_string(),
                "theme.components".to_string()
            ),
        ]
    );
    assert_eq!(
        resources.anonymous_component_paths,
        vec![
            ("ui".to_string(), PathBuf::from("/ws/resources/views/ui")),
            (
                String::new(),
                Path::new("/ws/app/Providers").join("../views/pkg")
            ),
        ]
    );
}

#[test]
fn records_registered_custom_directives() {
    // Both registration methods, on the facade and on the compiler
    // instance a deferred callback receives.  Only the name is recorded:
    // the handler returns PHP no static scan can evaluate.
    let content = "<?php\n\
        class AppServiceProvider {\n\
            public function boot(): void {\n\
                Blade::directive('datetime', fn ($e) => \"<?php echo ($e); ?>\");\n\
                Blade::if('admin', fn () => auth()->user()?->isAdmin());\n\
                $this->callAfterResolving('blade.compiler', function ($blade) {\n\
                    $blade->directive('money', fn ($e) => $e);\n\
                });\n\
                $this->app['blade.compiler']->if('subscribed', fn () => true);\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/app/Providers/AppServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(
        resources.custom_directives,
        vec![
            crate::blade::directives::CustomDirective {
                name: "datetime".to_string(),
                conditional: false,
            },
            crate::blade::directives::CustomDirective {
                name: "admin".to_string(),
                conditional: true,
            },
            crate::blade::directives::CustomDirective {
                name: "money".to_string(),
                conditional: false,
            },
            crate::blade::directives::CustomDirective {
                name: "subscribed".to_string(),
                conditional: true,
            },
        ]
    );
}

/// `directive` and `if` are names an unrelated fluent API can carry, so
/// a call on anything but Blade's compiler registers nothing.
#[test]
fn ignores_a_directive_call_on_an_unrelated_receiver() {
    let content = "<?php\n\
        class AppServiceProvider {\n\
            public function boot(): void {\n\
                $this->schedule->if('daily', fn () => true);\n\
                $builder->directive('weird', fn () => true);\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/app/Providers/AppServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert!(resources.custom_directives.is_empty());
}

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
    let resources = extract_provider_resources(
        content,
        file_path,
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
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
    let resources = extract_provider_resources(
        content,
        file_path,
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
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
    let resources = extract_provider_resources(
        content,
        file_path,
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
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
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/Provider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
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
    let resources = extract_provider_resources(
        content,
        file_path,
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(resources.config_files.len(), 1);
    assert_eq!(
        resources.config_files[0].path,
        Path::new("/ws/vendor/livewire/livewire/src").join("../config/livewire.php")
    );
    assert_eq!(resources.config_files[0].namespace, "livewire");
}

#[test]
fn config_registrations_require_a_key() {
    let content = "<?php\n\
        class AppServiceProvider {\n\
            public function register(): void {\n\
                $this->mergeConfigFrom();\n\
                $this->mergeConfigFrom(base_path('config/incomplete.php'));\n\
                $this->mergeConfigFrom(base_path('config/null.php'), null);\n\
                $this->mergeConfigFrom(base_path('config/bakery.php'), 'bakery');\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/app/Providers/AppServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(
        resources.config_files,
        vec![ProviderResource {
            path: PathBuf::from("/ws/config/bakery.php"),
            namespace: "bakery".to_string(),
        }]
    );
}

/// A registration may name its file through any of Laravel's path helpers,
/// and `lang_path()` follows the framework into `resources/lang` when a
/// project still keeps its translations there.
#[test]
fn registrations_resolve_every_path_helper_like_laravel() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("resources/lang")).unwrap();
    let content = "<?php\n\
        class AppServiceProvider {\n\
            public function boot(): void {\n\
                $this->mergeConfigFrom(config_path('billing.php'), 'billing');\n\
                $this->loadTranslationsFrom(lang_path('vendor/billing'), 'billing');\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        &workspace
            .path()
            .join("app/Providers/AppServiceProvider.php"),
        workspace.path(),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(
        resources.config_files,
        vec![ProviderResource {
            path: workspace.path().join("config/billing.php"),
            namespace: "billing".to_string(),
        }]
    );
    assert_eq!(
        resources.trans_dirs,
        vec![ProviderResource {
            path: workspace.path().join("resources/lang/vendor/billing"),
            namespace: "billing".to_string(),
        }]
    );
}

#[test]
fn records_the_config_keys_a_provider_writes() {
    // `laravel/sanctum` adds its guard this way rather than in a config
    // file it merges.  The read nested in the guard's value, and the
    // read after it, declare nothing.
    let content = "<?php\n\
        class SanctumServiceProvider {\n\
            public function register(): void {\n\
                config([\n\
                    'auth.guards.sanctum' => array_merge(['driver' => 'sanctum'], config('auth.guards.sanctum', [])),\n\
                ]);\n\
                Config::set('queue.connections.package', ['driver' => 'sync']);\n\
                config()->set('mail.mailers.package', []);\n\
                config('app.name');\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/vendor/laravel/sanctum/src/SanctumServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(
        resources.config_writes,
        [
            "auth.guards.sanctum",
            "queue.connections.package",
            "mail.mailers.package"
        ]
    );
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
        ClassContext::default(),
        Default::default(),
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
        ClassContext::default(),
        Default::default(),
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
        ClassContext::default(),
        Default::default(),
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
        ClassContext::default(),
        Default::default(),
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
        ClassContext::default(),
        Default::default(),
    );
    assert!(!resources.custom_translation_loader);
}

#[test]
fn records_view_namespaces_added_on_the_view_factory() {
    let content = "<?php\n\
        class ViewServiceProvider {\n\
            public function boot(): void {\n\
                View::addNamespace('webshop', base_path('resources/webshop/views'));\n\
                $this->app['view']->addNamespace('backoffice', [base_path('resources/backoffice/views')]);\n\
                View::prependNamespace('mail', base_path('resources/mail'));\n\
                $this->schedule->addNamespace('nope', base_path('x'));\n\
            }\n\
        }\n";
    let resources = extract_provider_resources(
        content,
        Path::new("/ws/app/Providers/ViewServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    let registered: Vec<_> = resources
        .view_dirs
        .iter()
        .map(|r| (r.namespace.as_str(), r.path.clone()))
        .collect();
    assert_eq!(
        registered,
        vec![
            ("mail", PathBuf::from("/ws/resources/mail")),
            ("webshop", PathBuf::from("/ws/resources/webshop/views")),
            (
                "backoffice",
                PathBuf::from("/ws/resources/backoffice/views")
            ),
        ]
    );
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
    let resources = extract_provider_resources(
        content,
        file_path,
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    assert_eq!(resources.config_files.len(), 1);
    assert!(
        resources.view_dirs.is_empty(),
        "an undefined `$path` in a different method must not resolve to another method's assignment"
    );
}

/// The fold table a provider whose base class declares
/// `public static $abstract = 'sentry';` is scanned with, as
/// `provider_class_context` builds it from the inheritance-merged class.
fn sentry_context() -> ClassContext {
    let mut class = crate::test_fixtures::make_class("ServiceProvider");
    let mut abstract_key = crate::test_fixtures::make_property("abstract", Some("string"));
    abstract_key.is_static = true;
    abstract_key.source = Some(crate::types::PropertySource::DeclaredDefault {
        value: "'sentry'".into(),
    });
    class.properties = vec![std::sync::Arc::new(abstract_key)].into();

    let mut version = crate::test_fixtures::make_constant("VERSION");
    version.value = Some("'4.0'".to_string());
    class.constants = vec![std::sync::Arc::new(version)].into();

    ClassContext::from_class(&class)
}

fn scan_sentry_provider(content: &str) -> ProviderResources {
    let mut resources = extract_provider_resources(
        content,
        Path::new("/ws/vendor/sentry/sentry-laravel/src/Sentry/Laravel/ServiceProvider.php"),
        Path::new("/ws"),
        sentry_context(),
        Default::default(),
    );
    resources.resolve_aliases();
    resources
}

#[test]
fn binds_a_key_named_by_an_inherited_static_property() {
    // Sentry declares the container key on the base provider and binds
    // under `static::$abstract` from the subclass the application
    // registers, so `app('sentry')` only resolves once the property folds.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton(static::$abstract, fn () => new HubAdapter());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
}

#[test]
fn binds_a_factory_that_configures_before_returning() {
    // A factory that sets the instance up before handing it back returns
    // a variable, not the instantiation itself; the concrete class is the
    // last value assigned to that variable above the return.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', function ($app) {\n\
                    $hub = new HubAdapter($app->make('sentry.loader'));\n\
                    $hub->setFallback($app->getFallbackLocale());\n\
                    return $hub;\n\
                });\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
}

#[test]
fn binds_a_factory_by_its_declared_return_type() {
    // The body builds nothing this scan can name, but the factory says
    // what it hands back.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', function ($app): HubAdapter {\n\
                    return $app->make('sentry.factory')->build();\n\
                });\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
}

#[test]
fn a_declared_return_type_beats_the_class_the_factory_body_builds() {
    // A declared return type decides what a call yields everywhere else,
    // and a container factory is no different: an author who writes the
    // contract means the contract, so binding the implementation behind it
    // would offer members the application never asked for.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', function (): HubInterface {\n\
                    return new HubAdapter();\n\
                });\n\
                $this->app->singleton('sentry.hub', fn (): HubInterface => new HubAdapter());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubInterface")
    );
    assert_eq!(
        resources
            .bindings
            .get("sentry.hub")
            .map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubInterface"),
        "an arrow function's hint decides the same way"
    );
}

/// A hint that names no single class leaves the body to decide, so the
/// flip above costs nothing where the declaration says nothing.
#[test]
fn a_factory_whose_hint_names_no_class_still_binds_from_its_body() {
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', fn (): static => new HubAdapter());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
}

#[test]
fn binds_the_string_keyed_entries_of_the_bindings_arrays() {
    // Laravel applies `$bindings` and `$singletons` itself, after the
    // provider's own `register()` has run.  A `Contract::class` key names
    // something that already resolves, and retyping it to the concrete
    // would contradict the contract the application declared.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public array $bindings = [\n\
                'sentry.hub' => HubAdapter::class,\n\
                HubInterface::class => HubAdapter::class,\n\
            ];\n\
            public $singletons = ['sentry.client' => Client::class];\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources
            .bindings
            .get("sentry.hub")
            .map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
    assert_eq!(
        resources
            .bindings
            .get("sentry.client")
            .map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\Client")
    );
    assert!(
        !resources
            .bindings
            .contains_key("Sentry\\Laravel\\HubInterface"),
        "a contract-keyed entry must not retype the contract"
    );
}

#[test]
fn a_bindings_array_entry_that_names_no_key_or_class_is_skipped() {
    // Every shape the arrays hold that says nothing bindable: a property
    // with no default, one holding something other than an array, the
    // list shorthand (whose entry binds a class to itself and so needs no
    // table), an integer key (Laravel's spelling of that same shorthand),
    // a key built at runtime, and a value that names no class.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public array $bindings;\n\
            public $singletons = self::DEFAULTS;\n\
            public array $aliases = ['ignored' => HubAdapter::class];\n\
            public array $extra = [\n\
                HubAdapter::class,\n\
                0 => HubAdapter::class,\n\
                $runtime => HubAdapter::class,\n\
                'no.class' => $factory,\n\
            ];\n\
        }\n";
    // The shapes above are spread over `$bindings`/`$singletons` and two
    // properties that are not read at all, so nothing may bind.
    let resources = scan_sentry_provider(content);
    let bound: Vec<&String> = resources.bindings.keys().collect();
    assert!(bound.is_empty(), "unexpected bindings: {bound:?}");

    // The same unbindable entries under a name that *is* read still bind
    // nothing, while the one good entry beside them does.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public array $bindings = [\n\
                HubAdapter::class,\n\
                0 => HubAdapter::class,\n\
                $runtime => HubAdapter::class,\n\
                'no.class' => $factory,\n\
                'sentry.hub' => HubAdapter::class,\n\
            ];\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.keys().collect::<Vec<_>>(),
        vec!["sentry.hub"]
    );
}

#[test]
fn a_provider_declared_inside_a_block_is_still_scanned() {
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        {\n\
            class ServiceProvider extends BaseServiceProvider {\n\
                public array $bindings = ['sentry.hub' => HubAdapter::class];\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources
            .bindings
            .get("sentry.hub")
            .map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
}

#[test]
fn an_arrow_function_factory_binds_by_its_declared_return_type() {
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', fn ($app): HubAdapter => $app->build());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
}

#[test]
fn a_factory_returning_something_that_is_not_a_class_binds_nothing() {
    // A built-in return type names no class the key could hold, and
    // guessing one would bind the key to something never resolved.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', fn ($app): array => $app->all());\n\
                $this->app->singleton('sentry.hub', function ($app) {\n\
                    return $app->build();\n\
                });\n\
            }\n\
        }\n";
    assert!(scan_sentry_provider(content).bindings.is_empty());
}

#[test]
fn an_alias_records_the_site_of_the_key_it_introduces() {
    // The alias is where *this* key is written, so it is what the key's
    // go-to-definition must land on — not the registration it points at.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', fn () => new HubAdapter());\n\
                $this->app->alias('sentry', 'sentry.hub');\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    let site = &resources.bindings.get("sentry.hub").expect("aliased").site;
    let offset = site.offset as usize;
    assert_eq!(&content[offset..offset + "sentry.hub".len()], "sentry.hub");
}

#[test]
fn a_key_that_is_not_a_literal_points_at_the_expression_that_names_it() {
    // `static::$abstract` has no quotes to land inside, so the key's
    // go-to-definition target is the expression itself.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton(static::$abstract, fn () => new HubAdapter());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    let site = &resources.bindings.get("sentry").expect("bound").site;
    let offset = site.offset as usize;
    let at_key = &content[offset..offset + "static::$abstract".len()];
    assert_eq!(at_key, "static::$abstract");
}

#[test]
fn a_bindings_array_replaces_what_register_bound() {
    // `Application::register()` applies the arrays after calling
    // `register()`, so the array is what the container ends up holding.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public array $bindings = ['sentry' => Client::class];\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', fn () => new HubAdapter());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\Client")
    );
}

#[test]
fn a_binding_records_where_its_key_is_written() {
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('sentry', fn () => new HubAdapter());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    let site = &resources.bindings.get("sentry").expect("bound").site;
    assert!(
        site.path.ends_with("Sentry/Laravel/ServiceProvider.php"),
        "unexpected registration file: {:?}",
        site.path
    );
    assert_eq!(
        &content[site.offset as usize..site.offset as usize + "sentry".len()],
        "sentry",
        "the offset should land on the key itself"
    );
}

#[test]
fn binds_a_key_built_from_a_class_constant() {
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton(self::VERSION . '.hub', fn () => new HubAdapter());\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("4.0.hub").map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter")
    );
}

#[test]
fn a_key_named_by_another_class_does_not_fold() {
    // `Unrelated::$abstract` names a class this scan never read; borrowing
    // the scanned provider's property of that name would bind the wrong key.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton(Unrelated::$abstract, fn () => new HubAdapter());\n\
            }\n\
        }\n";
    assert!(scan_sentry_provider(content).bindings.is_empty());
}

#[test]
fn alias_binds_its_second_argument_to_the_class_in_its_first() {
    // `alias()` takes its arguments the other way round from `bind()`.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        use Sentry\\State\\HubInterface;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->alias(HubInterface::class, static::$abstract);\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources.bindings.get("sentry").map(|b| b.class.as_str()),
        Some("Sentry\\State\\HubInterface")
    );
}

#[test]
fn alias_to_another_key_resolves_to_that_keys_concrete() {
    // Aliasing an already-bound string key is how a package exposes its
    // service under a contract name as well.
    let content = "<?php\n\
        namespace Sentry\\Laravel;\n\
        use Sentry\\State\\HubInterface;\n\
        class ServiceProvider extends BaseServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton(static::$abstract, fn () => new HubAdapter());\n\
                $this->app->alias(static::$abstract, HubInterface::class);\n\
            }\n\
        }\n";
    let resources = scan_sentry_provider(content);
    assert_eq!(
        resources
            .bindings
            .get("Sentry\\State\\HubInterface")
            .map(|b| b.class.as_str()),
        Some("Sentry\\Laravel\\HubAdapter"),
        "the contract name has to reach the class the aliased key is bound to"
    );
}

#[test]
fn an_alias_cycle_leaves_its_keys_unresolved() {
    let content = "<?php\n\
        class AppServiceProvider {\n\
            public function register(): void {\n\
                $this->app->alias('a', 'b');\n\
                $this->app->alias('b', 'a');\n\
            }\n\
        }\n";
    let mut resources = extract_provider_resources(
        content,
        Path::new("/ws/src/AppServiceProvider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Default::default(),
    );
    resources.resolve_aliases();
    assert!(resources.bindings.is_empty());
}

/// Scan a provider under the registration origin and parent chain that
/// decide the keys it shares with another provider.
fn scan_as(
    content: &str,
    fqn: &str,
    ancestors: &[&str],
    origin: ProviderOrigin,
) -> ProviderResources {
    extract_provider_resources(
        content,
        Path::new("/ws/src/Provider.php"),
        Path::new("/ws"),
        ClassContext::default(),
        Arc::new(ProviderIdentity {
            fqn: fqn.to_string(),
            ancestors: ancestors.iter().map(|a| a.to_string()).collect(),
            origin,
        }),
    )
}

const FRAMEWORK_TRANSLATION_PROVIDER: &str = "<?php\n\
    namespace Illuminate\\Translation;\n\
    class TranslationServiceProvider {\n\
        public function register(): void {\n\
            $this->app->singleton('translator', fn ($app) => new Translator($app));\n\
        }\n\
    }\n";

const APP_TRANSLATION_PROVIDER: &str = "<?php\n\
    namespace Acme\\Translation;\n\
    class TranslationServiceProvider extends \\Illuminate\\Translation\\TranslationServiceProvider {\n\
        public function register(): void {\n\
            $this->app->singleton('translator', fn ($app) => new DatabaseTranslator($app));\n\
        }\n\
    }\n";

#[test]
fn an_application_binding_beats_a_framework_default() {
    // Replacing a framework binding from an application provider is the
    // normal way to swap an implementation, and the two providers may be
    // scanned in either order, so neither order may hand the key back to
    // the framework.
    let framework = || {
        scan_as(
            FRAMEWORK_TRANSLATION_PROVIDER,
            "Illuminate\\Translation\\TranslationServiceProvider",
            &[],
            ProviderOrigin::Framework,
        )
    };
    let application = || {
        scan_as(
            APP_TRANSLATION_PROVIDER,
            "Acme\\Translation\\TranslationServiceProvider",
            &["Illuminate\\Translation\\TranslationServiceProvider"],
            ProviderOrigin::Application,
        )
    };

    let mut framework_first = ProviderResources::default();
    framework_first.merge(framework());
    framework_first.merge(application());

    let mut application_first = ProviderResources::default();
    application_first.merge(application());
    application_first.merge(framework());

    for resources in [framework_first, application_first] {
        assert_eq!(
            resources
                .bindings
                .get("translator")
                .map(|b| b.class.as_str()),
            Some("Acme\\Translation\\DatabaseTranslator"),
            "the application's registration decides the key"
        );
    }
}

#[test]
fn a_subclass_provider_beats_the_parent_it_extends() {
    let parent = "<?php\n\
        namespace Acme;\n\
        class ServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('acme.client', fn () => new Client());\n\
            }\n\
        }\n";
    let child = "<?php\n\
        namespace App\\Providers;\n\
        class AcmeServiceProvider extends \\Acme\\ServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('acme.client', fn () => new TracingClient());\n\
            }\n\
        }\n";

    // Both are registered the same way, and the subclass is scanned first,
    // so only its parent chain marks it as the later registration.
    let mut resources = ProviderResources::default();
    resources.merge(scan_as(
        child,
        "App\\Providers\\AcmeServiceProvider",
        &["Acme\\ServiceProvider"],
        ProviderOrigin::Application,
    ));
    resources.merge(scan_as(
        parent,
        "Acme\\ServiceProvider",
        &[],
        ProviderOrigin::Application,
    ));

    assert_eq!(
        resources
            .bindings
            .get("acme.client")
            .map(|b| b.class.as_str()),
        Some("App\\Providers\\TracingClient")
    );
}

#[test]
fn two_unrelated_providers_leave_the_key_to_the_later_one() {
    // Nothing ranks one above the other, and the container keeps whichever
    // registered last, which is the order they are scanned in.
    let first = "<?php\n\
        namespace A;\n\
        class ServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('shared', fn () => new First());\n\
            }\n\
        }\n";
    let second = "<?php\n\
        namespace B;\n\
        class ServiceProvider {\n\
            public function register(): void {\n\
                $this->app->singleton('shared', fn () => new Second());\n\
            }\n\
        }\n";

    let mut resources = ProviderResources::default();
    resources.merge(scan_as(
        first,
        "A\\ServiceProvider",
        &[],
        ProviderOrigin::Application,
    ));
    resources.merge(scan_as(
        second,
        "B\\ServiceProvider",
        &[],
        ProviderOrigin::Application,
    ));

    assert_eq!(
        resources.bindings.get("shared").map(|b| b.class.as_str()),
        Some("B\\Second")
    );
}

#[test]
fn a_replaced_translator_binds_the_key_to_the_replacement() {
    // Rebinding `translator` says both that the strings come from a source
    // we cannot enumerate *and* which class `app('translator')` hands back.
    let resources = scan_as(
        APP_TRANSLATION_PROVIDER,
        "Acme\\Translation\\TranslationServiceProvider",
        &["Illuminate\\Translation\\TranslationServiceProvider"],
        ProviderOrigin::Application,
    );
    assert!(resources.custom_translation_loader);
    assert_eq!(
        resources
            .bindings
            .get("translator")
            .map(|b| b.class.as_str()),
        Some("Acme\\Translation\\DatabaseTranslator")
    );
}
