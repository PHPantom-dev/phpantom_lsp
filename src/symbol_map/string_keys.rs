//! The string keys a symbol map records for framework lookups, and the
//! spans that only count while some workspace symbol is absent.

use crate::atom::Atom;

use super::{LaravelConfigResource, SymbolKind, SymbolMap, SymbolSpan};

/// Identifies the category of a [`SymbolKind::StringKey`] span.
///
/// One flat list for every framework: each variant names what its key
/// resolves against, so a framework's kinds sit beside the others rather
/// than behind a per-framework wrapper. [`Self::is_symfony`] says which
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
    /// A Yii view: the template a view renderer's `render()` family or a
    /// view's own `render()` loads, keyed by the path it names (see
    /// `yii::views`).
    YiiView,
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

    /// Whether this key belongs to Yii.
    pub(crate) const fn is_yii(self) -> bool {
        matches!(self, Self::YiiView)
    }

    /// Whether this key belongs to Symfony rather than to Laravel.
    pub(crate) const fn is_symfony(self) -> bool {
        matches!(
            self,
            Self::ServiceId
                | Self::ContainerParameter
                | Self::SymfonyRoute
                | Self::SymfonyTranslation
                | Self::TranslationDomain
                | Self::TwigTemplate
                | Self::SymfonyConfigKey
                | Self::SymfonyEvent
                | Self::MessengerBus
        )
    }
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
    /// The type of a parameter's value, generalized (`'en'` is a `string`).
    Type(crate::php_type::PhpType),
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
const SYMFONY_SERVICE_CONFIGURATOR: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\ServiceConfigurator";
const SYMFONY_PARAMETERS_CONFIGURATOR: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\ParametersConfigurator";
const SYMFONY_URL_GENERATOR: &str = "Symfony\\Component\\Routing\\Generator\\UrlGeneratorInterface";
const SYMFONY_ROUTING_CONFIGURATOR: &str =
    "Symfony\\Component\\Routing\\Loader\\Configurator\\RoutingConfigurator";
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

/// The namespace of the PHP configurators that build Symfony's service
/// container.
pub(crate) const SYMFONY_CONTAINER_CONFIGURATOR_NS: &str =
    "Symfony\\Component\\DependencyInjection\\Loader\\Configurator\\";
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
    /// parameter name instead.
    ContainerLookup,
    /// What `getParameter()`, `hasParameter()`, and `setParameter()` take a
    /// container parameter name in: the container itself, and the
    /// `getParameter()` shortcut a Symfony controller inherits.
    ParameterHolder,
    /// A PHP configurator's `set()`, which declares a service on the
    /// services configurator and a parameter on the parameters one.
    ConfiguratorSet,
    /// The services configurator, whose `alias()` declares a service ID
    /// for another one.
    ServicesConfigurator,
    /// One service's configurator, whose `decorate()` names the service it
    /// replaces.
    ServiceConfigurator,
    /// What `generateUrl()` and `redirectToRoute()` take a route name in:
    /// the shortcuts a Symfony controller inherits.
    ControllerRouting,
    /// A URL generator, the router included, whose `generate()` takes a
    /// route name.
    UrlGenerator,
    /// A PHP routing configurator, whose `add()` declares a route.
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
}

impl ReceiverClass {
    /// Each class the receiver's type can be a subtype of, most specific
    /// first, with the kind of key the call names when it is.
    pub(crate) fn targets(self) -> &'static [(&'static str, StringKeyKind)] {
        use StringKeyKind::{
            ContainerParameter, ServiceId, SymfonyEvent, SymfonyRoute, SymfonyTranslation,
            TranslationDomain, TwigTemplate, View, YiiView,
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
            Self::ServicesConfigurator => &[
                (SYMFONY_SERVICES_CONFIGURATOR, ServiceId),
                (SYMFONY_ABSTRACT_SERVICE_CONFIGURATOR, ServiceId),
            ],
            Self::ServiceConfigurator => &[(SYMFONY_SERVICE_CONFIGURATOR, ServiceId)],
            Self::ControllerRouting => &[(SYMFONY_ABSTRACT_CONTROLLER, SymfonyRoute)],
            Self::UrlGenerator => &[(SYMFONY_URL_GENERATOR, SymfonyRoute)],
            // Only the top-level configurator: a route a collection's
            // `add()` declares takes the collection's name prefix.
            Self::RoutingConfigurator => &[(SYMFONY_ROUTING_CONFIGURATOR, SymfonyRoute)],
            Self::Translator => &[(SYMFONY_TRANSLATOR, SymfonyTranslation)],
            Self::TranslatorDomain => &[(SYMFONY_TRANSLATOR, TranslationDomain)],
            Self::TemplateRenderer => &[
                (SYMFONY_ABSTRACT_CONTROLLER, TwigTemplate),
                (SYMFONY_CONTROLLER_HELPER, TwigTemplate),
                (TWIG_ENVIRONMENT, TwigTemplate),
            ],
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
        }
    }

    /// The namespace of the configurators the receiver has to be one of,
    /// when it is one of the configurators a PHP configuration file builds
    /// the container or the routes with.
    pub(crate) fn configurator_namespace(self) -> Option<&'static str> {
        match self {
            Self::ConfiguratorSet | Self::ServicesConfigurator | Self::ServiceConfigurator => {
                Some(SYMFONY_CONTAINER_CONFIGURATOR_NS)
            }
            Self::RoutingConfigurator => Some(SYMFONY_ROUTING_CONFIGURATOR_NS),
            _ => None,
        }
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

    /// The kind of key the call names on its first target, which says
    /// which framework the receiver belongs to.
    pub(crate) fn key_kind(self) -> StringKeyKind {
        self.targets()[0].1
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
    /// What the receiver has to be for this to name a key at all.
    pub receiver: ReceiverClass,
}

impl TypedReceiverSite {
    /// Whether this site, once confirmed, could name `key` as a `kind`.
    /// The reference index files a site under each kind it could become.
    pub(crate) fn may_name(&self, kind: StringKeyKind, key: &str) -> bool {
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
                is_optional: self.is_optional,
            },
        }
    }
}
