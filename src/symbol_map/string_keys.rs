//! The string keys a symbol map records for framework lookups, and the
//! spans that only count while some workspace symbol is absent.

use crate::atom::Atom;

use super::{LaravelConfigResource, SymbolKind, SymbolMap, SymbolSpan};

/// Identifies the category of a [`SymbolKind::StringKey`] span.
///
/// One flat list for every framework: each variant names what its key
/// resolves against, so a framework's kinds sit beside the others rather
/// than behind a per-framework wrapper. [`Self::framework`] says which
/// framework a variant belongs to.
///
/// Adding a new string-key navigation feature only requires adding a variant
/// here and updating the extraction and dispatch paths — the exhaustive
/// match arms in `highlight`, `hover`, `rename`, `semantic_tokens`, and
/// `type_definition` do not need to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StringKeyKind {
    /// A `config('dot.key')` or `Config::get('dot.key')` call.
    Config,
    /// A short name whose declaration is a direct child of a known config
    /// subtree, such as a storage disk or database connection.
    ConfigResource(LaravelConfigResource),
    /// A `view('name')` or `View::make('name')` call.
    View,
    /// A `route('name')` call.
    Route,
    /// A `__('key')`, `trans('key')`, or `Lang::get('key')` call.
    Trans,
    /// An Artisan command name, e.g. `Artisan::call('app:sync')`,
    /// `Schedule::command('app:sync')`, or `$this->call('app:sync')`
    /// inside a console command.
    Command,
    /// An Eloquent morph alias: the short name a `Relation::morphMap()`
    /// registration substitutes for a model FQCN in a `*_type` column.
    /// Emitted for the registration's own keys, for
    /// `Relation::getMorphedModel('post')`, and for the `$types` argument of
    /// the `whereHasMorph()` family.
    MorphAlias,
    /// A Blade section name: the string `@yield('content')` renders and
    /// `@section('content')` fills. Emitted for the marker calls the Blade
    /// preprocessor lowers those directives to.
    Section,
    /// A Blade stack name, the `@stack`/`@push` counterpart of
    /// [`Self::Section`].
    Stack,
    /// An authorization ability name: the string a `Gate::allows()`,
    /// `$user->can()`, `$this->authorize()`, `can:` middleware parameter, or
    /// Blade `@can` directive checks.  Resolves against `Gate::define()`
    /// registrations and the methods of the relevant model's policy.
    GateAbility,
    /// A service-container binding key: the string `app('payments')`,
    /// `resolve('payments')`, `App::make('payments')`, or
    /// `$this->app->make('payments')` asks the container for, and the one a
    /// service provider registers it under.
    ContainerBinding,
    /// A middleware alias or group name: the part before any `:` of a
    /// string `->middleware()`, `->withoutMiddleware()`, or the
    /// `#[Middleware]` controller attribute names, resolving against the
    /// aliases and groups the framework, the HTTP kernel, `bootstrap/app.php`,
    /// and the service providers register.
    Middleware,
    /// An environment variable name: the string `env('APP_NAME')` or
    /// `Env::get('APP_NAME')` reads, resolving to the line that declares it
    /// in the project's `.env`.
    Env,
    /// A Symfony service ID: the key a service is declared under in
    /// `services.yaml`, `services.xml`, or a PHP configurator's `->set()`,
    /// and the string `$container->get()`, `service()`, or
    /// `#[Autowire(service: …)]` asks for.
    ServiceId,
    /// A Symfony container parameter: declared under `parameters:` or with a
    /// PHP configurator's `$parameters->set()`, read by `getParameter()`,
    /// `param()`, `#[Autowire(param: …)]`, and `%name%` placeholders.
    ContainerParameter,
    /// A Symfony route name: declared by a `#[Route]` attribute, a key in
    /// the routing YAML, a `<route>` in the routing XML, or a routing
    /// configurator's `add()`, and read by `generateUrl()`,
    /// `redirectToRoute()`, a URL generator's `generate()`, and Twig's
    /// `path()` and `url()`.
    SymfonyRoute,
    /// A Symfony translation key, together with the domain it is looked up
    /// in (see `symfony::translations::translation_key`): declared by a YAML
    /// or XLIFF catalogue, and read by a translator's `trans()`, a
    /// `TranslatableMessage`, `t()`, and Twig's `trans` filter.
    SymfonyTranslation,
    /// A Symfony translation domain: the one a `trans()` call or a Twig
    /// `trans` filter names, or a template's `trans_default_domain`.  The
    /// catalogues named after it declare it.
    TranslationDomain,
    /// A Twig template name: declared by the template file itself (see
    /// `twig::templates`), and read by a controller's `render()` family,
    /// Twig's `render()`, a templated email's `htmlTemplate()` and
    /// `textTemplate()`, `#[Template]`, and a template's `extends`,
    /// `include`, and the other tags and functions that load one.
    TwigTemplate,
    /// A Twig block name: defined by a template's `{% block %}` tag, and
    /// rendered by `block()`.  A name only means something among the
    /// templates that extend each other (see `twig::block_navigation`).
    TwigBlock,
    /// A Twig filter name: declared by a `new TwigFilter('price', …)` or an
    /// `#[AsTwigFilter]` method in an extension, the project's or an
    /// installed package's (see `twig::callables`), and applied by a
    /// template's `|price`.
    TwigFilter,
    /// A Twig function name: declared like [`Self::TwigFilter`], with
    /// `TwigFunction` and `#[AsTwigFunction]`, and called by a template's
    /// `icon(…)`.
    TwigFunction,
    /// A variable a Twig template reads.  What a template receives is
    /// passed by the renders of it (see [`TwigContextSite`]) or set by the
    /// template itself, so a name only means something in the template it
    /// is written in (see `twig::variables`).  The project's globals are
    /// declared with this kind: a `twig.globals` key, an `addGlobal()`, or
    /// a `getGlobals()` key.
    TwigVariable,
    /// A bundle configuration key, as its dotted path from the bundle's
    /// root (`acme_demo.mailer.dsn`): declared by a `TreeBuilder` schema in
    /// a `getConfigTreeBuilder()` (see `symfony::config_tree`), and read by
    /// the keys under `config/packages/`.
    SymfonyConfigKey,
    /// A Symfony event name: declared by each listener that subscribes to
    /// it (an `#[AsEventListener]`, a `getSubscribedEvents()` key, a
    /// `kernel.event_listener` tag, or an event dispatcher's
    /// `addListener()`), and read by the dispatcher's `dispatch()` and the
    /// rest of its listener API.
    SymfonyEvent,
    /// A Symfony Messenger bus name: declared under
    /// `framework.messenger.buses`, and read by `default_bus`,
    /// `#[AsMessageHandler(bus: …)]`, and a `BusNameStamp`.
    MessengerBus,
    /// A Symfony serializer group: declared by each `#[Groups]` that puts a
    /// property or method in it and by the `groups` of a YAML or XML
    /// serializer mapping, and read by the `groups` of a serialization
    /// context (see `symfony::serializer`).
    SerializerGroup,
    /// A Yii view: the template a view renderer's `render()` family or a
    /// view's own `render()` loads, keyed by the path it names (see
    /// `yii::views`).
    YiiView,
    /// A Yii route name: declared by a `Route` definition's `->name()`,
    /// after the name prefixes of the groups around it, and read by a URL
    /// generator's `generate()`, `generateAbsolute()`, and
    /// `generateFromCurrent()`, and by the methods of a class of the
    /// project's own that pass a name on to one.
    YiiRoute,
    /// The route argument a handler's `#[RouteArgument('id')]` reads, keyed
    /// by the handler as well as the name (see
    /// `yii::routes::route_argument_key`): an `id` only means something
    /// under the routes that dispatch to that handler, whose paths declare
    /// it.
    YiiRouteArgument,
    /// A WordPress hook name: declared by each place that fires it (a
    /// `do_action()`, an `apply_filters()`, and their `_ref_array` and
    /// `_deprecated` forms), and read by `add_action()`, `add_filter()`, and
    /// the rest of the listener API. A name built at runtime
    /// (`"save_post_{$post->post_type}"`) is stored as a pattern (see
    /// `wordpress::hooks`), which a literal name matches by its literal
    /// parts.
    WordPressHook,
    /// A Drupal entity type ID: declared by the `id` of an entity class's
    /// `#[ContentEntityType]`-family attribute or annotation, and read by
    /// the entity type manager's `getStorage()` and the other methods that
    /// take one.
    DrupalEntityType,
    /// A Drupal route name: declared by a top-level key of a routing file,
    /// and read by `Url::fromRoute()`, `Link::createFromRoute()`, a form
    /// state's `setRedirect()`, a controller's or form's `redirect()`, and
    /// the URL generator, access manager, and route provider methods that
    /// take one.
    DrupalRoute,
    /// A parameter of a Drupal route, keyed by the route as well as the
    /// name (see `drupal::routing::route_parameter_key`): a `{node}` only
    /// means something under its route.  Declared by each placeholder of
    /// the route's path, and read by the keys of the parameters a route
    /// call or a links file's `route_parameters:` passes.
    DrupalRouteParameter,
    /// A Drupal hook name: declared by the `hook_<name>()` function that
    /// documents it in an `*.api.php` file and by the module handler's and
    /// theme manager's calls that invoke it, and read by the implementations
    /// a `#[Hook]` attribute or a procedural function's name make.
    DrupalHook,
    /// A Drupal permission name: declared by a top-level key of a
    /// permissions file, and read by an account's or a role's
    /// `hasPermission()`, the access result helpers, a route's
    /// `_permission` requirement, and an entity type's `admin_permission`.
    DrupalPermission,
    /// A Drupal configuration name: declared by a default configuration
    /// file's name and by a configuration schema entry (see
    /// `drupal::config`), and read by `\Drupal::config()`, the config
    /// factory, the `config()` shortcut of controllers and forms, a config
    /// form's `getEditableConfigNames()`, and a form element's
    /// `#config_target`.
    DrupalConfigName,
    /// A key of a Drupal configuration object, keyed by the object's name
    /// as well as the key (see `drupal::config_keys::config_key`):
    /// declared by the object's configuration schema, and read by a config
    /// object's `get()`, `set()`, and `clear()`, the part after the `:` of
    /// a `#config_target`, and a `ConfigTarget`'s property path.
    DrupalConfigKey,
    /// A Drupal render element name: declared by the ID of a class's
    /// `#[RenderElement]` or `#[FormElement]` attribute, or of the
    /// annotation older code uses, and read by the `#type` of a render
    /// array.
    DrupalRenderElement,
    /// A Drupal theme hook name: declared by a key of the array a
    /// `hook_theme()` implementation returns (see `drupal::theme_hooks`),
    /// and read by a render array's `#theme` and `#theme_wrappers`.  A
    /// suggestion (`node__article`) reads as the hook it falls back to.
    DrupalThemeHook,
    /// A variable of a Drupal theme hook, keyed by the hook as written as
    /// well as the name (see `drupal::theme_hooks::theme_variable_key`):
    /// declared by a key of the hook's `variables`, and read by the `#`
    /// keys of a render array whose `#theme` names the hook.
    DrupalThemeHookVariable,
}

impl StringKeyKind {
    /// Whether this key resolves through Laravel's config index.
    pub(crate) const fn is_config_backed(self) -> bool {
        matches!(self, Self::Config | Self::ConfigResource(_))
    }

    /// Whether this key is a Symfony translation key or domain.
    pub(crate) const fn is_symfony_translation(self) -> bool {
        matches!(self, Self::SymfonyTranslation | Self::TranslationDomain)
    }

    /// The framework whose declarations this key resolves against.
    ///
    /// Exhaustive on purpose: a new kind has to say which framework turns
    /// it on.
    pub(crate) const fn framework(self) -> KeyFramework {
        match self {
            Self::Config
            | Self::ConfigResource(_)
            | Self::View
            | Self::Route
            | Self::Trans
            | Self::Command
            | Self::MorphAlias
            | Self::Section
            | Self::Stack
            | Self::GateAbility
            | Self::ContainerBinding
            | Self::Middleware
            | Self::Env => KeyFramework::Laravel,
            Self::ServiceId | Self::ContainerParameter => KeyFramework::ServiceContainer,
            Self::SymfonyRoute
            | Self::SymfonyTranslation
            | Self::TranslationDomain
            | Self::SymfonyConfigKey
            | Self::SymfonyEvent
            | Self::MessengerBus
            | Self::SerializerGroup => KeyFramework::Symfony,
            Self::TwigTemplate
            | Self::TwigBlock
            | Self::TwigFilter
            | Self::TwigFunction
            | Self::TwigVariable => KeyFramework::Twig,
            Self::YiiView | Self::YiiRoute | Self::YiiRouteArgument => KeyFramework::Yii,
            Self::WordPressHook => KeyFramework::WordPress,
            Self::DrupalEntityType
            | Self::DrupalRoute
            | Self::DrupalRouteParameter
            | Self::DrupalHook
            | Self::DrupalPermission
            | Self::DrupalConfigName
            | Self::DrupalConfigKey
            | Self::DrupalRenderElement
            | Self::DrupalThemeHook
            | Self::DrupalThemeHookVariable => KeyFramework::Drupal,
        }
    }

    /// The bit a [`LookupGuards`] mask holds this kind under: the kinds a
    /// lookup of can be written to cope with the name not existing.
    pub(crate) const fn guard_bit(self) -> u8 {
        match self {
            Self::SymfonyRoute => LookupGuards::ROUTE,
            Self::TwigTemplate => LookupGuards::TEMPLATE,
            Self::ContainerParameter => LookupGuards::PARAMETER,
            _ => 0,
        }
    }
}

/// The kinds of name a lookup is written to do without, as a mask of
/// [`StringKeyKind::guard_bit`]s: one inside a `try` that catches the
/// exception a missing name throws, or one on a router or Twig environment
/// the file builds itself, whose names are its own.
pub(crate) struct LookupGuards;

impl LookupGuards {
    pub(crate) const ROUTE: u8 = 1;
    pub(crate) const TEMPLATE: u8 = 2;
    pub(crate) const PARAMETER: u8 = 4;

    /// The kinds a `catch` of the class `caught` (resolved, without the
    /// leading `\`) copes with a missing name of: the exception the lookup
    /// throws, or one of its parents.
    pub(crate) fn caught_by(caught: &str) -> u8 {
        const ROUTE: &[&str] = &[
            "Symfony\\Component\\Routing\\Exception\\RouteNotFoundException",
            "Symfony\\Component\\Routing\\Exception\\ExceptionInterface",
        ];
        const TEMPLATE: &[&str] = &["Twig\\Error\\LoaderError", "Twig\\Error\\Error"];
        const PARAMETER: &[&str] = &[
            "Symfony\\Component\\DependencyInjection\\Exception\\ParameterNotFoundException",
            "Symfony\\Component\\DependencyInjection\\Exception\\InvalidArgumentException",
            "Symfony\\Component\\DependencyInjection\\Exception\\ExceptionInterface",
            "Psr\\Container\\NotFoundExceptionInterface",
            "Psr\\Container\\ContainerExceptionInterface",
        ];
        let is = |names: &[&str]| names.iter().any(|name| caught.eq_ignore_ascii_case(name));
        if ["Throwable", "Exception"]
            .iter()
            .any(|name| caught.eq_ignore_ascii_case(name))
        {
            return Self::ROUTE | Self::TEMPLATE | Self::PARAMETER;
        }
        // A missing route or parameter is an `\InvalidArgumentException`,
        // which is a `\LogicException`.
        if ["InvalidArgumentException", "LogicException"]
            .iter()
            .any(|name| caught.eq_ignore_ascii_case(name))
        {
            return Self::ROUTE | Self::PARAMETER;
        }
        let mut guards = 0;
        if is(ROUTE) {
            guards |= Self::ROUTE;
        }
        if is(TEMPLATE) {
            guards |= Self::TEMPLATE;
        }
        if is(PARAMETER) {
            guards |= Self::PARAMETER;
        }
        guards
    }
}

/// Whether two spans over the same text are two declarations of a kind of
/// key, of different names, so that both are kept.
pub(crate) fn declares_another_name(kept: &SymbolSpan, next: &SymbolSpan) -> bool {
    matches!(
        (&kept.kind, &next.kind),
        (
            SymbolKind::StringKey {
                kind: kept_kind,
                key: kept_key,
                is_write: true,
                ..
            },
            SymbolKind::StringKey {
                kind: next_kind,
                key: next_key,
                is_write: true,
                ..
            },
        ) if kept_kind == next_kind && kept_key != next_key
    )
}

/// The framework a [`StringKeyKind`] belongs to, which is what decides
/// whether a project reads keys of that kind at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyFramework {
    Laravel,
    /// Symfony's service IDs and container parameters, which any project
    /// that builds a Symfony service container declares.
    ServiceContainer,
    /// The rest of Symfony: routes, translations, bundle configuration,
    /// events, and Messenger.
    Symfony,
    /// Twig's template, block, filter, function, and variable names,
    /// read in any project that uses Twig.
    Twig,
    Yii,
    /// WordPress's hook names, read wherever the hook functions are called.
    WordPress,
    /// Drupal's own declarations, read in a Drupal project.
    Drupal,
}

/// A workspace symbol whose presence suppresses an otherwise-valid framework
/// runtime fallback.
///
/// PHP first looks for a namespace-local function before falling back to a
/// global helper, and Laravel's root facade aliases only exist when no real
/// global class has claimed the same name. These dependencies are stored on
/// the handful of ambiguous string spans so edits to another file can toggle
/// the spans without reparsing either file or adding work to request paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StringKeyDependency {
    Function(Atom),
    Class(Atom),
}

impl StringKeyDependency {
    /// The namespace-local `auth` function that may prevent PHP from reaching
    /// Laravel's global helper.
    pub(crate) fn namespaced_auth(name: &str) -> Option<Self> {
        let (namespace, short) = name.rsplit_once('\\')?;
        (!namespace.is_empty() && short.eq_ignore_ascii_case("auth"))
            .then(|| Self::Function(crate::atom::ascii_lowercase_atom(name)))
    }

    /// A global class name Laravel may otherwise provide as a runtime facade
    /// alias. Fully-qualified facade classes are unambiguous and return none.
    pub(crate) fn root_facade(name: &str) -> Option<Self> {
        let name = name.trim_start_matches('\\');
        if name.contains('\\') {
            return None;
        }
        let is_alias = crate::symbol_map::laravel_resources::RESOURCE_FACADES
            .iter()
            .any(|alias| name.eq_ignore_ascii_case(alias))
            || match name.len() {
                3 => name.eq_ignore_ascii_case("App"),
                4 => {
                    name.eq_ignore_ascii_case("Gate")
                        || name.eq_ignore_ascii_case("Lang")
                        || name.eq_ignore_ascii_case("View")
                }
                5 => name.eq_ignore_ascii_case("Route"),
                6 => name.eq_ignore_ascii_case("Config"),
                7 => name.eq_ignore_ascii_case("Artisan"),
                8 => name.eq_ignore_ascii_case("Response") || name.eq_ignore_ascii_case("Schedule"),
                _ => false,
            };
        is_alias.then(|| Self::Class(crate::atom::ascii_lowercase_atom(name)))
    }
}

/// A string-key span whose meaning depends on a workspace symbol being
/// absent. The payload moves between this side table and [`SymbolMap::spans`]
/// as it becomes dormant/active, so an active candidate does not duplicate
/// its key string in memory.
#[derive(Debug, Clone)]
pub(crate) struct ConditionalStringKeySpan {
    pub dependency: StringKeyDependency,
    start: u32,
    end: u32,
    dormant_span: Option<SymbolSpan>,
}

impl ConditionalStringKeySpan {
    pub(crate) fn new(dependency: StringKeyDependency, span: SymbolSpan) -> Self {
        Self {
            dependency,
            start: span.start,
            end: span.end,
            dormant_span: Some(span),
        }
    }

    fn is_inserted(&self) -> bool {
        self.dormant_span.is_none()
    }

    /// Heap bytes owned by a dormant payload for the optional memory audit.
    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_heap(&self) -> usize {
        self.dormant_span
            .as_ref()
            .map_or(0, |span| span.kind.audit_heap())
    }
}

fn is_candidate_span(span: &SymbolSpan, candidate: &ConditionalStringKeySpan) -> bool {
    span.start == candidate.start
        && span.end == candidate.end
        && matches!(&span.kind, SymbolKind::StringKey { .. })
}

impl SymbolMap {
    /// Whether this map has a dormant span gated by one of `affected`.
    pub(crate) fn has_conditional_string_key_dependency(
        &self,
        affected: Option<&std::collections::HashSet<StringKeyDependency>>,
    ) -> bool {
        self.conditional_string_key_spans.iter().any(|candidate| {
            affected.is_none_or(|dependencies| dependencies.contains(&candidate.dependency))
        })
    }

    /// Whether reconciling the selected dependencies would add or remove at
    /// least one span. This lets the publication path avoid cloning an
    /// already-correct map merely to discover that it is unchanged.
    pub(crate) fn conditional_string_key_spans_need_refresh(
        &self,
        affected: Option<&std::collections::HashSet<StringKeyDependency>>,
        mut dependency_exists: impl FnMut(StringKeyDependency) -> bool,
    ) -> bool {
        self.conditional_string_key_spans.iter().any(|candidate| {
            affected.is_none_or(|dependencies| dependencies.contains(&candidate.dependency))
                && candidate.is_inserted() == dependency_exists(candidate.dependency)
        })
    }

    /// Reconcile dormant string-key spans against the current workspace symbol
    /// indexes. When `affected` is supplied, candidates with any other
    /// dependency are skipped, keeping cross-file edits proportional to the
    /// uncommon ambiguous call sites rather than to every span in a file.
    pub(crate) fn refresh_conditional_string_key_spans(
        &mut self,
        affected: Option<&std::collections::HashSet<StringKeyDependency>>,
        mut dependency_exists: impl FnMut(StringKeyDependency) -> bool,
    ) -> bool {
        let mut changed = false;
        let mut added = false;
        for candidate in &mut self.conditional_string_key_spans {
            if affected.is_some_and(|set| !set.contains(&candidate.dependency)) {
                continue;
            }
            let should_insert = !dependency_exists(candidate.dependency);
            if should_insert == candidate.is_inserted() {
                continue;
            }

            if should_insert {
                self.spans.push(
                    candidate
                        .dormant_span
                        .take()
                        .expect("a dormant candidate must own its span"),
                );
                added = true;
            } else {
                let existing = self
                    .spans
                    .iter()
                    .position(|span| is_candidate_span(span, candidate))
                    .expect("an active candidate must be present in the symbol map");
                candidate.dormant_span = Some(self.spans.remove(existing));
            }
            changed = true;
        }
        if added {
            self.spans.sort_by_key(|span| span.start);
        }
        if changed {
            self.member_access_indices.clear();
            for (index, span) in self.spans.iter().enumerate() {
                if let SymbolKind::MemberAccess { member_name, .. } = &span.kind {
                    self.member_access_indices
                        .entry(*member_name)
                        .or_default()
                        .push(index);
                }
            }
        }
        changed
    }
}

// ─── Messenger handlers ─────────────────────────────────────────────────────

/// A Symfony Messenger handler: a method an `#[AsMessageHandler]` registers
/// for a message class.  Symfony takes the message from the attribute's
/// `handles:`, else from the type of the method's first parameter, one
/// handler per class of a union.
///
/// Kept in a side table rather than as a span, since a handler is a fact
/// about a class rather than about a string at a cursor: the code lenses
/// that link a message and its handlers read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessengerHandlerSite {
    /// The handler class, fully qualified.
    pub handler: Atom,
    /// The method that handles the message.
    pub method: Atom,
    /// The message class, fully qualified.
    pub message: Atom,
}

/// A Twig filter or function an extension declares, and the PHP callable
/// behind it.
///
/// Kept beside the declaring [`SymbolKind::StringKey`] span, which only
/// holds the name: what a template's `|price` runs is a fact about the
/// declaration, read by the definition and hover of the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TwigCallableSite {
    /// [`StringKeyKind::TwigFilter`] or [`StringKeyKind::TwigFunction`].
    pub kind: StringKeyKind,
    pub name: Atom,
    /// The byte range of the name, inside its quotes.
    pub start: u32,
    pub end: u32,
    pub target: TwigCallableTarget,
}

/// The PHP callable a Twig filter or function runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TwigCallableTarget {
    /// A method, of the class fully qualified.
    Method { class: Atom, method: Atom },
    /// A function, fully qualified.
    Function(Atom),
    /// A closure, or a callable that names nothing (a filter Twig compiles
    /// itself, such as `raw`).
    Inline,
}

/// The variables one render of a Twig template hands it: the keys of a
/// `render()`'s context array, of a templated email's `context()`, or of
/// what a `#[Template]` action returns.
///
/// Keyed by the template name it renders, whose span (or typed receiver
/// site, for a `render()` only its receiver's type confirms) starts at
/// [`Self::template_start`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TwigContextSite {
    pub template_start: u32,
    /// The names passed, each with the byte range it is written at.
    pub vars: Vec<TwigContextVar>,
    /// The byte range of a context no literal spells out (`render('…',
    /// $params)`), whose names only its type tells.
    pub shape: Option<(u32, u32)>,
}

/// One variable a [`TwigContextSite`] passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TwigContextVar {
    pub name: Atom,
    pub start: u32,
    pub end: u32,
    pub value: TwigContextValue,
}

/// Where the value a [`TwigContextVar`] passes is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TwigContextValue {
    /// The expression spanning `start..end`.
    Expression { start: u32, end: u32 },
    /// The PHP variable of the same name, as it stands at `offset`: what a
    /// `compact()` copies, and an action's argument.
    Variable { offset: u32 },
}

// ─── Container declarations ─────────────────────────────────────────────────

/// What a Symfony service or container parameter declaration says about the
/// value it declares, keyed back to the declaration's span (or, for a PHP
/// configurator, its still-to-be-confirmed site) by `start`.
///
/// Kept beside the spans rather than in them, since only the declarations
/// that give a class, an alias, or a value have one, and only the type
/// engine reads it: `$container->get('app.mailer')` is the class the
/// service is built from, and `getParameter('app.sender')` the type of the
/// parameter's value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContainerValueSite {
    /// Byte offset of the declared name's first character, matching the
    /// declaration's [`SymbolKind::StringKey`] span or
    /// [`TypedReceiverSite`].
    pub start: u32,
    /// What the declaration gives.
    pub value: ContainerValue,
}

/// What a Drupal hook span is, keyed back to its [`SymbolKind::StringKey`]
/// span by `start`.  A hook span with no site is the documentation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HookSite {
    pub start: u32,
    pub role: HookRole,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HookRole {
    /// An implementation, for `extension`.  Whether that extension is a
    /// theme is known for a procedural function, whose file says, and
    /// looked up by name for a hook class.  An implementation Drupal does
    /// not collect (an install-time hook, an update function) only links
    /// to its documentation.
    Implementation {
        extension: Atom,
        is_theme: Option<bool>,
        collected: bool,
    },
    /// An invocation, limited to `extension` when it names one.
    Invocation { extension: Option<Atom> },
}

/// An invoking call inside one of the core methods that fire hooks under
/// computed names (see `drupal::computed_hooks`), with the shape of the
/// name it builds: `*` for each part not written out, and `_alter` after
/// an alter type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedHookSite {
    /// The method the call is in.
    pub method: Atom,
    /// The byte range of the call's hook argument.
    pub start: u32,
    pub end: u32,
    pub shape: String,
    /// The call is an alter call.
    pub alter: bool,
    /// The call reaches themes (a theme manager's) or modules (a module
    /// handler's), when its receiver says which.
    pub themes: Option<bool>,
}

/// What a Drupal entity type declaration gives its ID: the entity class,
/// and the storage handler that loads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntityTypeSite {
    /// Byte offset of the ID's first character, matching the declaration's
    /// [`SymbolKind::StringKey`] span.
    pub start: u32,
    /// The class the attribute or annotation is written on.
    pub class: crate::atom::Atom,
    /// The `storage` handler the declaration names, or the default storage
    /// of its kind of entity type; `None` when it names none and has no
    /// default.
    pub storage: Option<crate::atom::Atom>,
}

/// The class a Drupal plugin declaration is written on, such as a render
/// element's.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginClassSite {
    /// Byte offset of the plugin ID's first character, matching the
    /// declaration's [`SymbolKind::StringKey`] span.
    pub start: u32,
    /// The class the attribute or annotation is written on.
    pub class: crate::atom::Atom,
}

/// The value a container declaration gives, read by the kind of name it
/// turns out to declare.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ContainerValue {
    /// A class name, fully qualified: the class a service is built from, or
    /// a parameter's string value that could name one.  A whole `%name%`
    /// placeholder is the value of the parameter `name`, as a service's
    /// class and as a parameter's value alike.
    Class(Atom),
    /// The service an alias points at.
    Alias(Atom),
    /// The service a child definition inherits from, whose class it is
    /// built from unless it names one of its own.
    Parent(Atom),
    /// The type of a parameter's value, generalized (`'en'` is a `string`).
    Type(crate::php_type::PhpType),
}

// ─── Routes ─────────────────────────────────────────────────────────────────

/// A route a file declares, with what its name alone does not say: the path
/// it matches and the handler it dispatches to.
///
/// Kept beside the spans rather than in them, since a path's placeholders
/// only mean something under their route (every route has its own `id`),
/// and only the route files of a project hold one.  A named route is keyed
/// back to its name's declaration span by [`Self::name_start`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteSite {
    /// Byte offset of the route name's declaration span, when the route
    /// has a name.
    pub name_start: Option<u32>,
    /// The whole path, with the prefixes of the groups around it, or
    /// `None` when one of them is not written as a literal.
    pub path: Option<String>,
    /// The placeholders of the path the file spells out, each with the byte
    /// range of its name, the group prefixes' included.
    pub placeholders: Vec<RoutePlaceholder>,
    /// The handler the route dispatches to, as
    /// `yii::routes::handler_key` spells it, when it names one.
    pub handler: Option<String>,
}

/// A route a Drupal routing file or `#[Route]` declares, with what Drupal
/// reads from it besides its name to upcast its parameters (see
/// `drupal::upcasting`): the path, the controller and title callback, the
/// parameters its `options` declare, the values its `defaults` give, and
/// the entity type an `_entity_view` or `_entity_form` default names.
///
/// Kept beside the spans, which hold the name and the path's parameters,
/// since only the type engine and hover read the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DrupalRouteSite {
    pub name: String,
    pub path: String,
    /// The method the route dispatches to, as `Class::method` without the
    /// leading `\`: its `_form`'s `buildForm()`, else its `_controller`
    /// (a class alone is its `__invoke()`).  `None` for a controller
    /// written as `service:method`, whose class only the container knows.
    pub controller: Option<String>,
    /// The `_title_callback`, spelled like [`Self::controller`].
    pub title_callback: Option<String>,
    /// Each parameter `options.parameters` declares, with or without a
    /// `type`.
    pub parameters: Vec<DeclaredParameter>,
    /// Each `defaults` key that is not one of Drupal's own (`_controller`,
    /// `_title`, …), with whether its value is `NULL`.  Drupal converts a
    /// parameter only when the path or a default gives it a value.
    pub defaults: Vec<(String, bool)>,
    /// The entity type the `_entity_view` default names, else the
    /// `_entity_form` one.
    pub entity_type: Option<String>,
}

/// A parameter a route's `options.parameters` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclaredParameter {
    pub name: String,
    /// The `type` it declares, as written (`entity:node`).
    pub ty: Option<String>,
    /// Whether it names a `converter` or a `tempstore`, which convert it
    /// some other way than Drupal's entity converter.
    pub converted_otherwise: bool,
}

impl DrupalRouteSite {
    /// What `options.parameters` declares for the parameter `name`.
    pub(crate) fn declared(&self, name: &str) -> Option<&DeclaredParameter> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == name)
    }

    /// Whether Drupal gives the parameter `name` a value to convert: the
    /// path declares it, or a default gives it one.
    pub(crate) fn passes(&self, name: &str) -> bool {
        crate::drupal::routing::path_parameter_names(&self.path)
            .any(|placeholder| placeholder == name)
            || self.defaults.iter().any(|(default, _)| default == name)
    }

    /// Whether a default gives the parameter `name` the value `NULL`, which
    /// Drupal leaves as it is.
    pub(crate) fn defaults_to_null(&self, name: &str) -> bool {
        self.defaults
            .iter()
            .any(|(default, is_null)| *is_null && default == name)
    }
}

/// One `{name}` in a route's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutePlaceholder {
    pub name: String,
    pub start: u32,
    pub end: u32,
}

/// Symfony routes a file loads from somewhere else: the `resource:` of a
/// routing YAML entry, an XML `<import>`, or a PHP configurator's
/// `import()`.
///
/// A file that declares routes it cannot be read for (a `#[Route]` whose
/// name is a constant, a configurator collection whose prefix is not a
/// literal) records one with no `resource`, which is how an import that
/// cannot be followed reads as well: the names under its `name_prefix`
/// are unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteImport {
    /// What the import loads, as written, or `None` when it is not written
    /// as a literal.
    pub resource: Option<String>,
    /// The loader the import names with its `type`, as written.
    pub loader: Option<String>,
    /// What the import puts in front of the name of every route it loads.
    pub name_prefix: String,
}

// ─── String keys behind a typed receiver ────────────────────────────────────

const PSR_CONTAINER: &str = "Psr\\Container\\ContainerInterface";
const SYMFONY_CONTAINER: &str = "Symfony\\Component\\DependencyInjection\\ContainerInterface";
const SYMFONY_CONTAINER_BAG: &str =
    "Symfony\\Component\\DependencyInjection\\ParameterBag\\ContainerBagInterface";
const SYMFONY_PARAMETER_BAG: &str =
    "Symfony\\Component\\DependencyInjection\\ParameterBag\\ParameterBagInterface";
const SYMFONY_ABSTRACT_CONTROLLER: &str =
    "Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController";
const SYMFONY_SERVICES_CONFIGURATOR: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\ServicesConfigurator";
const SYMFONY_ABSTRACT_SERVICE_CONFIGURATOR: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\AbstractServiceConfigurator";
const SYMFONY_ABSTRACT_CONFIGURATOR: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\AbstractConfigurator";
const SYMFONY_CONTAINER_BUILDER: &str = "Symfony\\Component\\DependencyInjection\\ContainerBuilder";
const SYMFONY_PARAMETERS_CONFIGURATOR: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\ParametersConfigurator";
const SYMFONY_URL_GENERATOR: &str = "Symfony\\Component\\Routing\\Generator\\UrlGeneratorInterface";
const YII_URL_GENERATOR: &str = "Yiisoft\\Router\\UrlGeneratorInterface";
pub(crate) const SYMFONY_ROUTING_CONFIGURATOR: &str =
    "Symfony\\Component\\Routing\\Loader\\Configurator\\RoutingConfigurator";
const SYMFONY_COLLECTION_CONFIGURATOR: &str =
    "Symfony\\Component\\Routing\\Loader\\Configurator\\CollectionConfigurator";
const SYMFONY_ROUTE_CONFIGURATOR: &str =
    "Symfony\\Component\\Routing\\Loader\\Configurator\\RouteConfigurator";
const SYMFONY_TRANSLATOR: &str = "Symfony\\Contracts\\Translation\\TranslatorInterface";
const SYMFONY_CONTROLLER_HELPER: &str =
    "Symfony\\Bundle\\FrameworkBundle\\Controller\\ControllerHelper";
const TWIG_ENVIRONMENT: &str = "Twig\\Environment";
const SYMFONY_TEMPLATED_EMAIL: &str = "Symfony\\Bridge\\Twig\\Mime\\TemplatedEmail";
const SYMFONY_EVENT_DISPATCHER_CONTRACT: &str =
    "Symfony\\Contracts\\EventDispatcher\\EventDispatcherInterface";
const SYMFONY_EVENT_DISPATCHER: &str =
    "Symfony\\Component\\EventDispatcher\\EventDispatcherInterface";
const YII_VIEW_RENDERER: &str = "Yiisoft\\Yii\\View\\Renderer\\ViewRenderer";
const YII_WEB_VIEW_RENDERER: &str = "Yiisoft\\Yii\\View\\Renderer\\WebViewRenderer";
/// The renderer before `yiisoft/yii-view-renderer` 7 moved it.
const YII_LEGACY_VIEW_RENDERER: &str = "Yiisoft\\Yii\\View\\ViewRenderer";
const YII_VIEW: &str = "Yiisoft\\View\\ViewInterface";
pub(crate) const DRUPAL_ENTITY_TYPE_MANAGER: &str =
    "Drupal\\Core\\Entity\\EntityTypeManagerInterface";
const DRUPAL_URL_GENERATOR: &str = "Drupal\\Core\\Routing\\UrlGeneratorInterface";
const DRUPAL_FORM_STATE: &str = "Drupal\\Core\\Form\\FormStateInterface";
const DRUPAL_CONTROLLER_BASE: &str = "Drupal\\Core\\Controller\\ControllerBase";
const DRUPAL_FORM_BASE: &str = "Drupal\\Core\\Form\\FormBase";
const DRUPAL_ENTITY_CONTROLLER: &str = "Drupal\\Core\\Entity\\Controller\\EntityController";
const DRUPAL_ACCESS_MANAGER: &str = "Drupal\\Core\\Access\\AccessManagerInterface";
const DRUPAL_ROUTE_PROVIDER: &str = "Drupal\\Core\\Routing\\RouteProviderInterface";
pub(crate) const DRUPAL_MODULE_HANDLER: &str = "Drupal\\Core\\Extension\\ModuleHandlerInterface";
pub(crate) const DRUPAL_THEME_MANAGER: &str = "Drupal\\Core\\Theme\\ThemeManagerInterface";
pub(crate) const DRUPAL_CONFIG_FACTORY: &str = "Drupal\\Core\\Config\\ConfigFactoryInterface";
const DRUPAL_CONFIG_BASE: &str = "Drupal\\Core\\Config\\ConfigBase";
const DRUPAL_CONFIG: &str = "Drupal\\Core\\Config\\Config";
const DRUPAL_ACCOUNT: &str = "Drupal\\Core\\Session\\AccountInterface";
const DRUPAL_ROLE: &str = "Drupal\\user\\RoleInterface";
const DRUPAL_PERMISSION_CHECKER: &str = "Drupal\\Core\\Session\\PermissionCheckerInterface";
const DRUPAL_CALCULATED_PERMISSIONS_ITEM: &str =
    "Drupal\\Core\\Session\\CalculatedPermissionsItemInterface";

/// The namespace of the PHP configurators that build Symfony's service
/// container.
pub(crate) const SYMFONY_CONTAINER_CONFIGURATOR_NS: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\";
const SYMFONY_SERIALIZER: &str = "Symfony\\Component\\Serializer\\SerializerInterface";
const SYMFONY_NORMALIZER: &str = "Symfony\\Component\\Serializer\\Normalizer\\NormalizerInterface";
const SYMFONY_DENORMALIZER: &str =
    "Symfony\\Component\\Serializer\\Normalizer\\DenormalizerInterface";
/// The namespace of the PHP configurators that build Symfony's routes.
pub(crate) const SYMFONY_ROUTING_CONFIGURATOR_NS: &str =
    "Symfony\\Component\\Routing\\Loader\\Configurator\\";

/// The classes a receiver has to be an instance of for the call it receives
/// to name a string key, and which kind of key each one makes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverClass {
    /// Laravel's view factory, whose `make()` / `first()` / `exists()` /
    /// `renderWhen()` / `renderUnless()` / `renderEach()` all name a
    /// template.
    ViewFactory,
    /// A mailable, whose `text()` names the plain-text template the message
    /// is built from.
    Mailable,
    /// Anything a mail template is set on: a mailable, or the mail message a
    /// notification's `toMail()` returns.  The two share no base class, but
    /// `view()` and `markdown()` mean the same thing on both.
    MailTemplate,
    /// What `get()` and `has()` look a name up in: a service container, or
    /// one of Symfony's parameter bags, whose same two methods take a
    /// parameter name instead, or Drupal's config factory, whose `get()`
    /// takes a configuration name, or a Drupal config object, whose `get()`
    /// takes one of its keys; the object it is comes from its type (see
    /// `drupal::types`).
    ContainerLookup,
    /// What `getParameter()`, `hasParameter()`, and `setParameter()` take a
    /// container parameter name in: the container itself, and the
    /// `getParameter()` shortcut a Symfony controller inherits.
    ParameterHolder,
    /// A PHP configurator's `set()`, which declares a service on the
    /// services configurator and a parameter on the parameters one.
    ConfiguratorSet,
    /// A PHP configurator's `alias()`, which declares another name for a
    /// service on the services configurator and for a route on a routing
    /// one.
    ConfiguratorAlias,
    /// One service's configurator, whose `decorate()` names the service it
    /// replaces and whose `parent()` the one it inherits its definition
    /// from.
    ServiceConfigurator,
    /// Any of the container's configurators, whose arguments, bindings,
    /// properties, and parameter values may hold `%name%` placeholders.
    ConfiguratorArgument,
    /// The container builder a kernel, a bundle, an extension, or a
    /// compiler pass defines services on: `register()`, `autowire()`,
    /// `setDefinition()`, and `setAlias()` declare a service ID, and the
    /// rest of its definition API looks one up, `getDefinition()` and
    /// `hasDefinition()` through [`Self::DefinitionLookup`].
    ContainerBuilder,
    /// What `generateUrl()` and `redirectToRoute()` take a route name in:
    /// the shortcuts a Symfony controller inherits.
    ControllerRouting,
    /// A URL generator, the router included, whose `generate()` takes a
    /// route name: Symfony's, Drupal's, whose `generateFromRoute()` takes
    /// one too, and Yii's, whose `generateAbsolute()` takes one too and
    /// whose `generateFromCurrent()` takes a fallback.
    UrlGenerator,
    /// A PHP routing configurator, a collection of one, or the route one
    /// of them just added, whose `add()` declares a route.
    RoutingConfigurator,
    /// A translator, whose `trans()` takes a translation key.
    Translator,
    /// A translator again, for the domain its `trans()` takes.
    TranslatorDomain,
    /// What renders a Twig template by name: a Symfony controller's
    /// `render()` family, and Twig's own environment.
    TemplateRenderer,
    /// A templated email, whose `htmlTemplate()` and `textTemplate()` name
    /// the templates its bodies are rendered from.
    TemplatedEmail,
    /// Twig's environment, whose `addGlobal()` declares a variable every
    /// template receives.
    TwigGlobals,
    /// An event dispatcher, whose `dispatch()` takes the event's name after
    /// the event itself.
    EventDispatcher,
    /// An event dispatcher that keeps listeners, whose `addListener()`
    /// subscribes one to an event name and whose `removeListener()`,
    /// `hasListeners()`, `getListeners()`, and `getListenerPriority()`
    /// look listeners up by it.
    EventListeners,
    /// What renders a Yii view by its first argument: a view renderer's
    /// `render()` family, and a view's own `render()` inside a template.
    YiiViewRenderer,
    /// Drupal's entity type manager, whose `getStorage()` and the other
    /// handler getters take an entity type ID first, `getDefinition()` and
    /// `hasDefinition()` through [`Self::DefinitionLookup`].
    EntityTypeManager,
    /// The definition lookups, `getDefinition()` and `hasDefinition()`, that
    /// both Symfony's container builder and Drupal's entity type manager
    /// have: the one takes a service ID, the other an entity type ID.
    DefinitionLookup,
    /// A Drupal form state, whose `setRedirect()` takes a route name.
    FormRedirect,
    /// What has Drupal's `redirect()` shortcut, which takes a route name: a
    /// controller or a form.
    RouteRedirect,
    /// Drupal's access manager, whose `checkNamedRoute()` takes a route
    /// name.
    AccessCheck,
    /// Drupal's route provider, whose `getRouteByName()` takes a route
    /// name.
    RouteLookup,
    /// A Drupal form state again, for the route parameters its
    /// `setRedirect()` takes after the route name.
    FormRedirectParameters,
    /// What has Drupal's `redirect()` shortcut again, for the route
    /// parameters it takes after the route name.
    RouteRedirectParameters,
    /// Drupal's URL generator, for the route parameters its
    /// `generateFromRoute()` and `generate()` take after the route name.
    UrlGeneratorParameters,
    /// Drupal's access manager, for the route parameters its
    /// `checkNamedRoute()` takes after the route name.
    AccessCheckParameters,
    /// Drupal's module handler, whose `invokeAll()`, `alter()`, and the
    /// rest invoke the hooks of modules and install profiles.
    ModuleHooks,
    /// Drupal's theme manager, whose `invoke()`, `alter()`, and the rest
    /// invoke the hooks of themes.
    ThemeHooks,
    /// Either of the two, for the methods they share.
    Hooks,
    /// What `hasPermission()` takes a permission name first on: an
    /// account, a role, the permission checker, and a calculated
    /// permissions item.
    PermissionCheck,
    /// A Drupal role, whose `grantPermission()` and `revokePermission()`
    /// take a permission name.
    RolePermissions,
    /// Drupal's config factory, whose `getEditable()` takes a
    /// configuration name and `loadMultiple()` a list of them; its `get()`
    /// is a [`Self::ContainerLookup`].
    ConfigFactory,
    /// What has Drupal's `config()` shortcut, which takes a configuration
    /// name: a controller or a form, a config form included.
    ConfigShortcut,
    /// An editable Drupal config object, whose `set()` and `clear()` take
    /// one of its keys; an `ImmutableConfig` throws on both.  Its `get()`
    /// is a [`Self::ContainerLookup`], as every config object's is.
    EditableConfigKeys,
    /// Symfony's serializer, normalizers, and denormalizers, whose
    /// `serialize()`, `deserialize()`, `normalize()`, and `denormalize()`
    /// take a context whose `groups` name serializer groups.
    Serializer,
    /// The `json()` shortcut a Symfony controller inherits, whose context
    /// does too.
    ControllerJson,
}

impl ReceiverClass {
    /// Each class the receiver's type can be a subtype of, most specific
    /// first, with the kind of key the call names when it is.
    pub(crate) fn targets(self) -> &'static [(&'static str, StringKeyKind)] {
        use StringKeyKind::{
            ContainerParameter, ServiceId, SymfonyEvent, SymfonyRoute, SymfonyTranslation,
            TranslationDomain, TwigTemplate, TwigVariable, View, YiiRoute, YiiView,
        };
        match self {
            // The concrete `Illuminate\View\Factory` implements the
            // contract, so the contract covers both spellings.
            Self::ViewFactory => &[("Illuminate\\Contracts\\View\\Factory", View)],
            Self::Mailable => &[("Illuminate\\Mail\\Mailable", View)],
            Self::MailTemplate => &[
                ("Illuminate\\Mail\\Mailable", View),
                ("Illuminate\\Notifications\\Messages\\MailMessage", View),
            ],
            // A container bag is a PSR container too, so it has to be asked
            // about first.
            Self::ContainerLookup => &[
                (DRUPAL_CONFIG_FACTORY, StringKeyKind::DrupalConfigName),
                (DRUPAL_CONFIG_BASE, StringKeyKind::DrupalConfigKey),
                (SYMFONY_CONTAINER_BAG, ContainerParameter),
                (SYMFONY_PARAMETER_BAG, ContainerParameter),
                (PSR_CONTAINER, ServiceId),
            ],
            Self::ParameterHolder => &[
                (SYMFONY_CONTAINER, ContainerParameter),
                (SYMFONY_ABSTRACT_CONTROLLER, ContainerParameter),
            ],
            Self::ConfiguratorSet => &[
                (SYMFONY_PARAMETERS_CONFIGURATOR, ContainerParameter),
                (SYMFONY_SERVICES_CONFIGURATOR, ServiceId),
                (SYMFONY_ABSTRACT_SERVICE_CONFIGURATOR, ServiceId),
            ],
            Self::ConfiguratorAlias => &[
                (SYMFONY_SERVICES_CONFIGURATOR, ServiceId),
                (SYMFONY_ABSTRACT_SERVICE_CONFIGURATOR, ServiceId),
                (SYMFONY_ROUTING_CONFIGURATOR, SymfonyRoute),
                (SYMFONY_COLLECTION_CONFIGURATOR, SymfonyRoute),
                (SYMFONY_ROUTE_CONFIGURATOR, SymfonyRoute),
            ],
            // `decorate()` and `parent()` are only on the configurators of
            // one service, so the base class settles it.
            Self::ServiceConfigurator => &[(SYMFONY_ABSTRACT_CONFIGURATOR, ServiceId)],
            Self::ConfiguratorArgument => &[(SYMFONY_ABSTRACT_CONFIGURATOR, ContainerParameter)],
            Self::ContainerBuilder => &[(SYMFONY_CONTAINER_BUILDER, ServiceId)],
            Self::ControllerRouting => &[(SYMFONY_ABSTRACT_CONTROLLER, SymfonyRoute)],
            // Drupal's generator extends Symfony's, so it has to be asked
            // about first.
            Self::UrlGenerator => &[
                (DRUPAL_URL_GENERATOR, StringKeyKind::DrupalRoute),
                (SYMFONY_URL_GENERATOR, SymfonyRoute),
                (YII_URL_GENERATOR, YiiRoute),
            ],
            // A collection's name prefix is read with the call (see
            // `record_symfony_receiver_sites`), and the site's key holds it.
            Self::RoutingConfigurator => &[
                (SYMFONY_ROUTING_CONFIGURATOR, SymfonyRoute),
                (SYMFONY_COLLECTION_CONFIGURATOR, SymfonyRoute),
                (SYMFONY_ROUTE_CONFIGURATOR, SymfonyRoute),
            ],
            Self::Translator => &[(SYMFONY_TRANSLATOR, SymfonyTranslation)],
            Self::TranslatorDomain => &[(SYMFONY_TRANSLATOR, TranslationDomain)],
            Self::TemplateRenderer => &[
                (SYMFONY_ABSTRACT_CONTROLLER, TwigTemplate),
                (SYMFONY_CONTROLLER_HELPER, TwigTemplate),
                (TWIG_ENVIRONMENT, TwigTemplate),
            ],
            Self::TwigGlobals => &[(TWIG_ENVIRONMENT, TwigVariable)],
            Self::TemplatedEmail => &[(SYMFONY_TEMPLATED_EMAIL, TwigTemplate)],
            // The component's dispatcher extends the contract, so the
            // contract covers both.
            Self::EventDispatcher => &[(SYMFONY_EVENT_DISPATCHER_CONTRACT, SymfonyEvent)],
            Self::EventListeners => &[(SYMFONY_EVENT_DISPATCHER, SymfonyEvent)],
            Self::YiiViewRenderer => &[
                (YII_WEB_VIEW_RENDERER, YiiView),
                (YII_VIEW_RENDERER, YiiView),
                (YII_LEGACY_VIEW_RENDERER, YiiView),
                (YII_VIEW, YiiView),
            ],
            Self::EntityTypeManager => {
                &[(DRUPAL_ENTITY_TYPE_MANAGER, StringKeyKind::DrupalEntityType)]
            }
            Self::DefinitionLookup => &[
                (SYMFONY_CONTAINER_BUILDER, ServiceId),
                (DRUPAL_ENTITY_TYPE_MANAGER, StringKeyKind::DrupalEntityType),
            ],
            Self::FormRedirect => &[(DRUPAL_FORM_STATE, StringKeyKind::DrupalRoute)],
            Self::RouteRedirect => &[
                (DRUPAL_CONTROLLER_BASE, StringKeyKind::DrupalRoute),
                (DRUPAL_FORM_BASE, StringKeyKind::DrupalRoute),
                (DRUPAL_ENTITY_CONTROLLER, StringKeyKind::DrupalRoute),
            ],
            Self::AccessCheck => &[(DRUPAL_ACCESS_MANAGER, StringKeyKind::DrupalRoute)],
            Self::RouteLookup => &[(DRUPAL_ROUTE_PROVIDER, StringKeyKind::DrupalRoute)],
            Self::FormRedirectParameters => {
                &[(DRUPAL_FORM_STATE, StringKeyKind::DrupalRouteParameter)]
            }
            Self::RouteRedirectParameters => &[
                (DRUPAL_CONTROLLER_BASE, StringKeyKind::DrupalRouteParameter),
                (DRUPAL_FORM_BASE, StringKeyKind::DrupalRouteParameter),
                (
                    DRUPAL_ENTITY_CONTROLLER,
                    StringKeyKind::DrupalRouteParameter,
                ),
            ],
            Self::UrlGeneratorParameters => {
                &[(DRUPAL_URL_GENERATOR, StringKeyKind::DrupalRouteParameter)]
            }
            Self::AccessCheckParameters => {
                &[(DRUPAL_ACCESS_MANAGER, StringKeyKind::DrupalRouteParameter)]
            }
            Self::ModuleHooks => &[(DRUPAL_MODULE_HANDLER, StringKeyKind::DrupalHook)],
            Self::ThemeHooks => &[(DRUPAL_THEME_MANAGER, StringKeyKind::DrupalHook)],
            Self::Hooks => &[
                (DRUPAL_MODULE_HANDLER, StringKeyKind::DrupalHook),
                (DRUPAL_THEME_MANAGER, StringKeyKind::DrupalHook),
            ],
            Self::PermissionCheck => &[
                (DRUPAL_ACCOUNT, StringKeyKind::DrupalPermission),
                (DRUPAL_ROLE, StringKeyKind::DrupalPermission),
                (DRUPAL_PERMISSION_CHECKER, StringKeyKind::DrupalPermission),
                (
                    DRUPAL_CALCULATED_PERMISSIONS_ITEM,
                    StringKeyKind::DrupalPermission,
                ),
            ],
            Self::RolePermissions => &[(DRUPAL_ROLE, StringKeyKind::DrupalPermission)],
            Self::ConfigFactory => &[(DRUPAL_CONFIG_FACTORY, StringKeyKind::DrupalConfigName)],
            Self::EditableConfigKeys => &[(DRUPAL_CONFIG, StringKeyKind::DrupalConfigKey)],
            Self::ConfigShortcut => &[
                (DRUPAL_CONTROLLER_BASE, StringKeyKind::DrupalConfigName),
                (DRUPAL_FORM_BASE, StringKeyKind::DrupalConfigName),
            ],
            Self::Serializer => &[
                (SYMFONY_SERIALIZER, StringKeyKind::SerializerGroup),
                (SYMFONY_NORMALIZER, StringKeyKind::SerializerGroup),
                (SYMFONY_DENORMALIZER, StringKeyKind::SerializerGroup),
            ],
            Self::ControllerJson => {
                &[(SYMFONY_ABSTRACT_CONTROLLER, StringKeyKind::SerializerGroup)]
            }
        }
    }

    /// The namespaces of the configurators the receiver can be one of, when
    /// it is one of the configurators a PHP configuration file builds the
    /// container or the routes with.  Empty for any other receiver.
    pub(crate) fn configurator_namespaces(self) -> &'static [&'static str] {
        match self {
            Self::ConfiguratorSet | Self::ServiceConfigurator | Self::ConfiguratorArgument => {
                &[SYMFONY_CONTAINER_CONFIGURATOR_NS]
            }
            Self::RoutingConfigurator => &[SYMFONY_ROUTING_CONFIGURATOR_NS],
            Self::ConfiguratorAlias => &[
                SYMFONY_CONTAINER_CONFIGURATOR_NS,
                SYMFONY_ROUTING_CONFIGURATOR_NS,
            ],
            _ => &[],
        }
    }

    /// Whether a call on the receiver that declares a name can declare a
    /// route: a routing configurator's `add()` and `alias()`.
    pub(crate) fn declares_routes(self) -> bool {
        matches!(self, Self::RoutingConfigurator | Self::ConfiguratorAlias)
    }

    /// Whether a call on a class of the project's own that passes the
    /// argument on to this receiver names a key too.  Only a URL generator
    /// is wrapped that way, and a call's method is read for it, so only
    /// one is followed.
    pub(crate) fn follows_wrappers(self) -> bool {
        self == Self::UrlGenerator
    }

    /// Whether the call names a template.
    pub(crate) fn names_views(self) -> bool {
        matches!(
            self,
            Self::ViewFactory | Self::Mailable | Self::MailTemplate
        )
    }

    /// Whether the call names a Twig template.
    pub(crate) fn names_twig_templates(self) -> bool {
        matches!(self, Self::TemplateRenderer | Self::TemplatedEmail)
    }

    /// Whether the call can name a key in a project for which `enabled`
    /// holds of some kind of key one of its targets names.
    pub(crate) fn names_any(self, enabled: impl Fn(StringKeyKind) -> bool) -> bool {
        self.targets().iter().any(|(_, kind)| enabled(*kind))
    }
}

/// A method call that names a string key, but whose meaning lives in its
/// receiver's *type* rather than its spelling: a constructor-injected
/// `Factory $views` behind `$this->views->make('page')`, a mailable held in
/// a local behind `$mail->view('emails.shipped')`, a `ContainerInterface
/// $container` behind `$container->get('app.mailer')`.
///
/// [`extract_symbol_map`](super::extraction) runs during `update_ast`,
/// before the file's classes are resolved, and cannot ask the forward walker
/// what the receiver is.  It records the site instead, and
/// `Backend::typed_receiver_spans_for` settles it lazily.
#[derive(Debug, Clone)]
pub(crate) struct TypedReceiverSite {
    /// Byte offset of the first character of the string's contents,
    /// matching what a [`SymbolKind::StringKey`] span records.
    pub start: u32,
    /// Byte offset one past the last character of those contents.
    pub end: u32,
    /// The key, in the spelling its kind stores (a view name dotted).
    pub key: String,
    /// Whether the call declares the key rather than reading it.
    pub is_write: bool,
    /// Whether the call does without the key naming anything, as a
    /// `first()` render of several candidate templates does.
    pub is_optional: bool,
    /// The kinds of key the call is written to do without, as a mask of
    /// [`StringKeyKind::guard_bit`]s (see [`LookupGuards`]).
    pub guards: u8,
    /// What the receiver has to be for this to name a key at all.
    pub receiver: ReceiverClass,
}

impl TypedReceiverSite {
    /// Whether this site, once confirmed, could name `key` as a `kind`.
    /// The reference index files a site under each kind it could become.
    pub(crate) fn may_name(&self, kind: StringKeyKind, key: &str) -> bool {
        // A configuration key's site holds the key alone; the object's
        // name comes with its receiver's type.
        let key = if kind == StringKeyKind::DrupalConfigKey {
            crate::drupal::config_keys::split_config_key(key).1
        } else {
            key
        };
        self.key == key
            && self
                .receiver
                .targets()
                .iter()
                .any(|(_, target)| *target == kind)
    }

    /// The span this site contributes once its receiver is confirmed to
    /// name a `kind`.
    pub(crate) fn to_span(&self, kind: StringKeyKind) -> SymbolSpan {
        SymbolSpan {
            start: self.start,
            end: self.end,
            kind: SymbolKind::StringKey {
                key: self.key.clone(),
                kind,
                is_write: self.is_write,
                is_optional: self.is_optional || self.guards & kind.guard_bit() != 0,
            },
        }
    }
}
