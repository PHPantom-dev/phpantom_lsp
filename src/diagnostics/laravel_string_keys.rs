//! The checks that judge a string key, and the command-signature check that
//! shares their shape.
//!
//! Both read the spans the symbol map recorded for a file, resolve each name
//! against what the project actually registers (routes, config keys,
//! views, translation keys, console commands, morph aliases, gate
//! abilities, Symfony service IDs, container parameters, routes and Twig
//! templates, Yii views, routes, and the route arguments a handler reads),
//! and report the ones nothing declares.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use super::helpers;
use crate::Backend;
use crate::class_lookup::Fqn;

/// The [`crate::symbol_map::StringKeyKind`]s
/// [`Backend::collect_invalid_laravel_string_key_diagnostics`] can judge.
///
/// The kinds it cannot are dropped when the spans are gathered rather than
/// carried to the check and skipped there, so the reason each one is left
/// alone is written once: a Blade section or stack name is judged by the
/// Blade pass, which knows the templates around the one it is written in,
/// and a container binding key is judged by nothing at all, since anything
/// can be bound at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckedStringKind {
    Route,
    Config,
    ConfigResource(crate::symbol_map::LaravelConfigResource),
    View,
    Trans,
    Command,
    MorphAlias,
    GateAbility,
    ServiceId,
    ContainerParameter,
    SymfonyRoute,
    TwigTemplate,
    MessengerBus,
    YiiView,
    YiiRoute,
    YiiRouteArgument,
}

impl CheckedStringKind {
    /// The kind to judge a key of `kind` as, or `None` for the kinds this
    /// pass leaves alone.
    fn of(kind: &crate::symbol_map::StringKeyKind) -> Option<Self> {
        use crate::symbol_map::StringKeyKind;

        Some(match kind {
            StringKeyKind::Route => Self::Route,
            StringKeyKind::Config => Self::Config,
            StringKeyKind::ConfigResource(resource) => Self::ConfigResource(*resource),
            StringKeyKind::View => Self::View,
            StringKeyKind::Trans => Self::Trans,
            StringKeyKind::Command => Self::Command,
            StringKeyKind::MorphAlias => Self::MorphAlias,
            StringKeyKind::GateAbility => Self::GateAbility,
            StringKeyKind::ServiceId => Self::ServiceId,
            StringKeyKind::ContainerParameter => Self::ContainerParameter,
            StringKeyKind::SymfonyRoute => Self::SymfonyRoute,
            StringKeyKind::TwigTemplate => Self::TwigTemplate,
            StringKeyKind::MessengerBus => Self::MessengerBus,
            StringKeyKind::YiiView => Self::YiiView,
            StringKeyKind::YiiRoute => Self::YiiRoute,
            StringKeyKind::YiiRouteArgument => Self::YiiRouteArgument,
            // A section or stack name is judged against the templates that
            // render the one it is written in, which the Blade pass has and
            // this one does not.  And anything at all can be bound at
            // runtime, so an unrecognised container key proves nothing —
            // nor does an environment variable absent from `.env`, since
            // the environment a process runs with is not on disk.  A
            // translation key no catalogue of the project declares may be
            // one an installed bundle's does, or a message that is its own
            // translation.  A configuration key is only declared by the
            // project's own schemas so far, which say nothing about what
            // a key under an open node or an installed bundle may be.  An
            // event is declared by the listeners that subscribe to it, and
            // dispatching one nobody listens to is no mistake.
            StringKeyKind::SymfonyTranslation
            | StringKeyKind::TranslationDomain
            | StringKeyKind::Section
            | StringKeyKind::Stack
            | StringKeyKind::ContainerBinding
            | StringKeyKind::Env
            | StringKeyKind::SymfonyConfigKey
            | StringKeyKind::SymfonyEvent => return None,
        })
    }
}

/// A key the pass judges: its kind, the key, and its byte range.
type CheckedKeySpan<'a> = (CheckedStringKind, &'a str, u32, u32);

/// What a key check concluded: `None` when the key cannot be judged, else
/// whether the project declares it, with the label and diagnostic code an
/// undeclared one is reported under.
type Verdict = Option<(bool, &'static str, &'static str)>;

/// What the project declares for the kinds of key one file names, read
/// once per pass and only for those kinds.  The enumerations are shared
/// with every other consumer, so they are read in place rather than copied
/// into a set per pass.  A kind that cannot be judged gets an empty (or
/// `None`) answer, which every check below reads as "skip".
struct DeclaredKeys<'a> {
    backend: &'a Backend,
    /// `None` unless the project registers routes of its own.
    routes: Option<Arc<crate::virtual_members::laravel::RouteDiscovery>>,
    config_keys: Arc<[String]>,
    /// Config namespaces a provider registers but whose file we could not
    /// read: their keys are real, and none of them can be judged.
    unread_config_namespaces: Vec<String>,
    /// A provider registered config under a namespace we could not name.
    open_config_namespace: bool,
    /// The config resources (guards, disks, …) whose subtree the declared
    /// config keys cover.
    config_resource_mask: u8,
    view_names: Arc<[String]>,
    trans_keys: Arc<[String]>,
    commands_indexed: bool,
    morph_map_enforced: bool,
    gate_abilities: Arc<[String]>,
    /// The service IDs the project's configuration declares, sorted.
    services: Vec<String>,
    /// The container parameters the project's configuration declares,
    /// sorted.
    parameters: Vec<String>,
    /// The Symfony route names the project declares, sorted.
    symfony_routes: Vec<String>,
    /// The Twig templates the project holds, sorted.
    twig_templates: Vec<String>,
    /// The Messenger buses the project's configuration declares, sorted.
    messenger_buses: Vec<String>,
    /// The Yii route names the project declares, sorted.
    yii_routes: Vec<String>,
}

impl<'a> DeclaredKeys<'a> {
    /// Enumerate what the project declares for the kinds in `spans`.
    ///
    /// Must not run under the file-state read lock: the enumerations
    /// call `user_file_symbol_maps()` → `ensure_workspace_indexed()` →
    /// `parse_files_parallel()` → `update_ast()`, which acquires a WRITE
    /// lock on `Backend::files`.
    fn enumerate(backend: &'a Backend, spans: &[CheckedKeySpan]) -> Self {
        let names = |wanted: fn(CheckedStringKind) -> bool| spans.iter().any(|span| wanted(span.0));
        let has_route = names(|kind| kind == CheckedStringKind::Route);
        let has_config_resource =
            names(|kind| matches!(kind, CheckedStringKind::ConfigResource(_)));
        let has_config = has_config_resource || names(|kind| kind == CheckedStringKind::Config);
        let has_view = names(|kind| kind == CheckedStringKind::View);
        let has_trans = names(|kind| kind == CheckedStringKind::Trans);
        let has_command = names(|kind| kind == CheckedStringKind::Command);
        let has_morph_alias = names(|kind| kind == CheckedStringKind::MorphAlias);
        let has_gate_ability = names(|kind| kind == CheckedStringKind::GateAbility);
        let has_service = names(|kind| kind == CheckedStringKind::ServiceId);
        let has_parameter = names(|kind| kind == CheckedStringKind::ContainerParameter);
        let has_symfony_route = names(|kind| kind == CheckedStringKind::SymfonyRoute);
        let has_twig_template = names(|kind| kind == CheckedStringKind::TwigTemplate);
        let has_messenger_bus = names(|kind| kind == CheckedStringKind::MessengerBus);
        let has_yii_route = names(|kind| kind == CheckedStringKind::YiiRoute);

        // A package with no routes of its own, whose names are registered
        // by the host application, cannot be judged: the valid set is
        // unknown, not empty.  Installed packages register routes of their
        // own, so the question is whether *this project* contributed any,
        // not whether the set is empty.
        let routes = has_route
            .then(|| backend.cached_routes())
            .filter(|discovery| discovery.routes.iter().any(|route| !route.from_vendor));

        // A library is installed into an application that declares the
        // configuration it reads, and that application is a file we never
        // see, so none of its keys can be judged.  Only an application owns
        // the whole of its configuration.
        let has_config = has_config && backend.is_application_project();
        if has_config {
            // Runtime writes can live in unopened files. Index them before
            // judging reads, outside the config-enumeration build locks. A
            // completed index is reused as is: watched-file notifications
            // keep it current, and a refresh walk per diagnostic pass would
            // put a workspace scan on every keystroke.
            backend.ensure_workspace_index_ready_for_request();
        }
        let config_keys: Arc<[String]> = if has_config {
            backend.cached_config_keys()
        } else {
            Arc::default()
        };
        let (unread_config_namespaces, open_config_namespace) = if has_config {
            let resources = backend.laravel.provider_resources.read();
            let unread = resources
                .config_files
                .iter()
                .filter(|file| !file.path.is_file())
                .map(|file| file.namespace.clone())
                .chain(resources.unread_config_namespaces.iter().cloned())
                .collect();
            (unread, resources.open_config_namespace)
        } else {
            (Vec::new(), false)
        };
        let config_resource_mask = if has_config_resource {
            config_keys.iter().fold(0, |mask, key| {
                crate::symbol_map::laravel_resources::resource_from_config_key(key)
                    .map_or(mask, |(resource, _)| mask | resource.bit())
            })
        } else {
            0
        };

        let view_names: Arc<[String]> = if has_view {
            backend.cached_view_names()
        } else {
            Arc::default()
        };

        // An application whose strings live in a database still has `vendor/`'s
        // own `lang/` files on disk, so the enumerated set is non-empty while
        // covering none of the application's own keys.  Once a provider has
        // rebound the translator away from Laravel's file loader, what is valid
        // is unknowable.
        let trans_keys: Arc<[String]> = if has_trans
            && !backend
                .laravel
                .provider_resources
                .read()
                .custom_translation_loader
        {
            backend.cached_trans_keys()
        } else {
            Arc::default()
        };

        // When no commands were indexed at all, skip command diagnostics
        // entirely.  The scan is heuristic (it relies on the `*Command`
        // naming convention), so an empty index likely means discovery
        // failed rather than that every referenced command is invalid.
        let commands_indexed = has_command && !backend.laravel.commands.read().is_empty();

        // A morph alias is only checkable when the project calls
        // `Relation::enforceMorphMap()` / `requireMorphMap()`.  Without that,
        // an unmapped model still morphs under its class name, so the set of
        // valid `*_type` values is open and an unknown alias proves nothing.
        let morph_map_enforced = has_morph_alias && backend.laravel.morph_map.read().is_enforced();

        // Abilities are only checkable when the project defines some: an
        // empty set means gate discovery found nothing (a project that
        // authorizes entirely through runtime-registered callbacks, or one
        // that is not really using Laravel's gate), not that every ability
        // referenced is wrong.
        //
        // A `Gate::before()` callback, or a package that answers checks from a
        // permission table, grants abilities that appear nowhere in source.  A
        // single unrelated `Gate::define()` call is enough to make the
        // enumerated set non-empty, so emptiness alone does not catch this: the
        // ability space is open and the whole check has to stand down —
        // including the walk of every policy class that would enumerate it.
        let gate_abilities: Arc<[String]> =
            if has_gate_ability && !backend.laravel.gates.read().ability_space_is_open() {
                backend.cached_gate_abilities()
            } else {
                Arc::default()
            };

        let services = if has_service {
            backend.symfony_declared_names(crate::symbol_map::StringKeyKind::ServiceId)
        } else {
            Vec::new()
        };
        let parameters = if has_parameter {
            backend.symfony_declared_names(crate::symbol_map::StringKeyKind::ContainerParameter)
        } else {
            Vec::new()
        };
        let symfony_routes = if has_symfony_route {
            backend.symfony_declared_names(crate::symbol_map::StringKeyKind::SymfonyRoute)
        } else {
            Vec::new()
        };
        let twig_templates = if has_twig_template {
            backend.symfony_declared_names(crate::symbol_map::StringKeyKind::TwigTemplate)
        } else {
            Vec::new()
        };

        let messenger_buses = if has_messenger_bus {
            backend.symfony_declared_names(crate::symbol_map::StringKeyKind::MessengerBus)
        } else {
            Vec::new()
        };

        let yii_routes = if has_yii_route {
            backend.symfony_declared_names(crate::symbol_map::StringKeyKind::YiiRoute)
        } else {
            Vec::new()
        };

        Self {
            backend,
            routes,
            config_keys,
            unread_config_namespaces,
            open_config_namespace,
            config_resource_mask,
            view_names,
            trans_keys,
            commands_indexed,
            morph_map_enforced,
            gate_abilities,
            services,
            parameters,
            symfony_routes,
            twig_templates,
            messenger_buses,
            yii_routes,
        }
    }

    fn judge(&self, kind: CheckedStringKind, key: &str) -> Verdict {
        match kind {
            CheckedStringKind::Route => self.judge_route(key),
            CheckedStringKind::Config => self.judge_config_key(key),
            CheckedStringKind::ConfigResource(resource) => {
                self.judge_config_resource(resource, key)
            }
            CheckedStringKind::View => Some((
                self.view_names
                    .binary_search_by(|name| name.as_str().cmp(key))
                    .is_ok(),
                "view",
                "invalid_laravel_view",
            )),
            CheckedStringKind::Trans => {
                // When no translation files are found at all, skip trans
                // diagnostics entirely.  This avoids false positives in
                // non-Laravel projects (WordPress, GetText) that also use
                // `__()` or `trans()` as function names.
                if self.trans_keys.is_empty() {
                    return None;
                }
                Some((
                    names_key_or_group(&self.trans_keys, key),
                    "translation key",
                    "invalid_laravel_trans",
                ))
            }
            CheckedStringKind::Command => self.commands_indexed.then(|| {
                (
                    self.backend.laravel.commands.read().contains_name(key),
                    "command",
                    "invalid_laravel_command",
                )
            }),
            CheckedStringKind::MorphAlias => self.morph_map_enforced.then(|| {
                (
                    self.backend.laravel.morph_map.read().has_alias(key),
                    "morph type",
                    "invalid_laravel_morph_alias",
                )
            }),
            // An ability is reported with its own message, see
            // `Backend::gate_ability_problem`.
            CheckedStringKind::GateAbility => None,
            CheckedStringKind::ServiceId => self.judge_service(key),
            CheckedStringKind::ContainerParameter => {
                Self::judge_application_key(&self.parameters, "app.", key)
                    .map(|valid| (valid, "container parameter", "invalid_symfony_parameter"))
            }
            CheckedStringKind::SymfonyRoute => {
                Self::judge_application_key(&self.symfony_routes, "app_", key)
                    .map(|valid| (valid, "route", "invalid_symfony_route"))
            }
            CheckedStringKind::TwigTemplate => self.judge_twig_template(key),
            CheckedStringKind::MessengerBus => self.judge_messenger_bus(key),
            // A view whose alias the project does not declare cannot be
            // told missing.
            CheckedStringKind::YiiView => self
                .backend
                .yii_view_exists(key)
                .map(|exists| (exists, "view", "invalid_yii_view")),
            CheckedStringKind::YiiRoute => self.judge_yii_route(key),
            CheckedStringKind::YiiRouteArgument => self.judge_yii_route_argument(key),
        }
    }

    /// No package Yii installs declares routes of its own, so the
    /// project's routes are all of them.  A project that declares none was
    /// not read at all.
    fn judge_yii_route(&self, key: &str) -> Verdict {
        if self.yii_routes.is_empty() {
            return None;
        }
        let valid = self
            .yii_routes
            .binary_search_by(|name| name.as_str().cmp(key))
            .is_ok();
        Some((valid, "route", "invalid_yii_route"))
    }

    /// A route argument is judged against the paths of the routes that
    /// dispatch to its handler, when the project declares one that does and
    /// its path is written out.
    fn judge_yii_route_argument(&self, key: &str) -> Verdict {
        let (handler, name) = crate::yii::routes::split_route_argument_key(key);
        let declared = self.backend.yii_handler_arguments(handler)?;
        Some((
            declared.iter().any(|declared| declared == name),
            "route argument",
            "invalid_yii_route_argument",
        ))
    }

    /// A bus is only ever declared by the application's own Messenger
    /// configuration, so a name it does not declare is wrong.  A project
    /// that declares none was not read at all (its configuration may be
    /// PHP), and its one bus is the default one besides.
    fn judge_messenger_bus(&self, key: &str) -> Verdict {
        if self.messenger_buses.is_empty() {
            return None;
        }
        let valid = self
            .messenger_buses
            .binary_search_by(|name| name.as_str().cmp(key))
            .is_ok();
        Some((valid, "Messenger bus", "invalid_symfony_messenger_bus"))
    }

    /// A template under a Twig namespace (`@Acme/…`) is a bundle's, whose
    /// own templates are not read, so only the project's own templates are
    /// judged.  A project with none was not read at all.
    /// A name under a Twig namespace is registered at runtime, and a name
    /// can only be told unknown once some root is known, and when it ends
    /// in an extension the project's templates use.
    fn judge_twig_template(&self, key: &str) -> Verdict {
        if key.starts_with('@') || self.twig_templates.is_empty() {
            return None;
        }
        let roots = self.backend.twig_roots();
        if roots.is_empty() || !roots.is_template_name(key) {
            return None;
        }
        let valid = self
            .twig_templates
            .binary_search_by(|name| name.as_str().cmp(key))
            .is_ok();
        Some((valid, "template", "invalid_symfony_template"))
    }

    /// A service ID spelled as a class name is that class, which an
    /// autowired service needs no declaration to be: it is wrong only when
    /// no such class exists.
    fn judge_service(&self, key: &str) -> Verdict {
        let valid = if key.contains('\\') {
            self.services
                .binary_search_by(|name| name.as_str().cmp(key))
                .is_ok()
                || self.backend.find_or_load_class(key).is_some()
        } else {
            Self::judge_application_key(&self.services, "app.", key)?
        };
        Some((valid, "service", "invalid_symfony_service"))
    }

    /// Whether `key`, a service ID, parameter, or route name, is among the
    /// `declared` ones, when that can be told at all.
    ///
    /// The configuration read is the project's own: the services,
    /// parameters, and routes the bundles it installs register are not, and
    /// neither are the ones a compiler pass or a route loader adds.  Symfony
    /// names an application's own under the `app.` prefix (`app_` for a
    /// route), which no bundle uses, so only those can be judged.  A project
    /// that declares none was not read at all.
    fn judge_application_key(declared: &[String], prefix: &str, key: &str) -> Option<bool> {
        if !key.starts_with(prefix) || declared.is_empty() {
            return None;
        }
        Some(
            declared
                .binary_search_by(|name| name.as_str().cmp(key))
                .is_ok(),
        )
    }

    fn judge_route(&self, key: &str) -> Verdict {
        let discovery = self.routes.as_ref()?;
        // A group whose `->name()` argument was not a string literal
        // (e.g. Filament's `Route::name($panelId . '.')` has children we
        // cannot enumerate statically.  Any route that falls under such a
        // prefix is unjudgeable, and so is any route ending in one of the
        // names such a group registers, even when it recorded no known
        // prefix at all (e.g. a group with no enclosing literal group whose
        // own name is entirely a variable).
        if discovery
            .open_prefixes
            .iter()
            .any(|prefix| key.starts_with(prefix))
            || discovery
                .open_suffixes
                .iter()
                .any(|suffix| key.ends_with(suffix))
        {
            return None;
        }
        // A `Route::is('admin.*')` check names a pattern rather than one
        // route, and matches whatever the project has under it.
        let valid = if key.contains('*') {
            discovery
                .names
                .iter()
                .any(|name| crate::virtual_members::laravel::route_name_matches(key, name))
        } else {
            discovery
                .names
                .binary_search_by(|name| name.as_str().cmp(key))
                .is_ok()
        };
        Some((valid, "route", "invalid_laravel_route"))
    }

    fn judge_config_key(&self, key: &str) -> Verdict {
        // An application owns its whole configuration: its own `config/`
        // files, the framework's defaults, and the files its providers
        // merge.  A key under a root none of them declares is as wrong as a
        // typo inside one of them, unless a provider registers config the
        // enumeration could not read.  A root that only runtime writes
        // establish is not judged either: the keys one file writes say
        // nothing about what the rest of that namespace holds, least of all
        // when the writes that established it were spelled dynamically.
        if self.config_keys.is_empty()
            || self
                .unread_config_namespaces
                .iter()
                .any(|namespace| is_key_or_under(key, namespace))
        {
            return None;
        }
        let root = key.split('.').next().unwrap_or(key);
        if !names_key_or_group(&self.config_keys, root)
            && (self.open_config_namespace || self.backend.runtime_config_key_covers(root))
        {
            return None;
        }
        // Config keys may be partial prefixes (e.g. `config('app')`) which
        // are valid even without a direct match.  A key that
        // `Config::set()` or the array form of the `config()` helper
        // establishes is as real as one a `config/` file declares; a test
        // that configures a disk in `setUp()` before exercising it is the
        // common shape.
        let valid = names_key_or_group(&self.config_keys, key)
            || self.backend.runtime_config_key_covers(key);
        Some((valid, "config key", "invalid_laravel_config"))
    }

    fn judge_config_resource(
        &self,
        resource: crate::symbol_map::LaravelConfigResource,
        key: &str,
    ) -> Verdict {
        if crate::symbol_map::laravel_resources::is_implicit_resource_name(resource, key) {
            return None;
        }
        // An undiscovered subtree is an unknown vocabulary, not proof that
        // every runtime-provided name is invalid.
        if self.config_resource_mask & resource.bit() == 0 {
            return None;
        }
        let descriptor = crate::symbol_map::laravel_resources::descriptor(resource);
        let config_key = crate::symbol_map::laravel_resources::config_key(resource, key);
        let valid = self.config_keys.binary_search(&config_key).is_ok()
            || self.backend.runtime_config_key_covers(&config_key);
        Some((valid, descriptor.label, descriptor.diagnostic_code))
    }
}

impl Backend {
    /// Emit a warning for each `$this->argument('x')` / `$this->option('x')`
    /// whose name is not a parameter of the enclosing command's `$signature`.
    pub(super) fn collect_invalid_command_param_diagnostics(
        &self,
        ctx: &helpers::FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        use crate::symbol_map::SymbolKind;

        // (name, is_option, start, end) for each own-param span.
        let spans: Vec<(String, bool, u32, u32)> = ctx
            .symbol_map
            .spans
            .iter()
            .filter(|span| super::incremental::reports_site(span.start))
            .filter_map(|span| {
                if let SymbolKind::CommandOwnParam { name, is_option } = &span.kind {
                    Some((name.clone(), *is_option, span.start, span.end))
                } else {
                    None
                }
            })
            .collect();
        if spans.is_empty() {
            return;
        }

        // The spans of one command class all share its signature, so it is
        // resolved on the first of them and reused until a span falls outside
        // that class' body.
        let mut enclosing: Option<crate::virtual_members::laravel::EnclosingCommand> = None;

        for (name, is_option, start, end) in &spans {
            if !enclosing
                .as_ref()
                .is_some_and(|command| command.body.contains(start))
            {
                enclosing = crate::virtual_members::laravel::command_enclosing_signature(
                    content,
                    *start as usize,
                );
            }
            // A class that declares no `$signature` (e.g. a `$name`-only or
            // dynamically-built command) has nothing to validate against.
            let Some(signature) = enclosing.as_ref().and_then(|c| c.signature.as_ref()) else {
                continue;
            };
            let known = if *is_option {
                signature.option(name).is_some()
            } else {
                signature.argument(name).is_some()
            };
            if !known
                && let Some(range) =
                    self.offset_range_to_lsp_range(uri, content, *start as usize, *end as usize)
            {
                let label = if *is_option { "option" } else { "argument" };
                out.push(helpers::make_diagnostic(
                    range,
                    DiagnosticSeverity::Warning,
                    "invalid_command_parameter",
                    format!("Unknown command {}: '{}'", label, name),
                ));
            }
        }
    }

    /// Emit a warning for each `StringKey` span whose key does
    /// not resolve to any declaration (typo in route name, config key,
    /// view name, or translation key).
    ///
    /// Only the kinds this pass can judge reach the check itself; see
    /// [`CheckedStringKind`].
    pub(super) fn collect_invalid_laravel_string_key_diagnostics(
        &self,
        ctx: &helpers::FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        use crate::symbol_map::SymbolKind;

        // The pass holds the map by `Arc`, not under the file-state read
        // lock, so enumerating declarations below can take the write lock
        // (see `DeclaredKeys::enumerate`) without deadlocking.
        let symbol_map = &ctx.symbol_map;
        let extra = self.typed_receiver_spans_for(uri, symbol_map);
        let morph_columns = self.morph_column_spans_for(uri, symbol_map);
        let key_spans: Vec<CheckedKeySpan> = symbol_map
            .spans
            .iter()
            .chain(extra.iter())
            .chain(morph_columns.iter())
            .filter(|span| super::incremental::reports_site(span.start))
            .filter_map(|span| {
                let SymbolKind::StringKey {
                    kind,
                    key,
                    is_write,
                    is_optional,
                } = &span.kind
                else {
                    return None;
                };
                // A write declares the key it names, so there is nothing
                // to check it against, and an optional key is one the call
                // is written to do without: an `@includeFirst` candidate
                // that names nothing is why the directive takes a list at
                // all.
                if *is_write || *is_optional || !self.string_key_kind_enabled(*kind) {
                    return None;
                }
                Some((
                    CheckedStringKind::of(kind)?,
                    key.as_str(),
                    span.start,
                    span.end,
                ))
            })
            .collect();
        if key_spans.is_empty() {
            return;
        }

        let declared = DeclaredKeys::enumerate(self, &key_spans);
        let range = |start: u32, end: u32| {
            self.offset_range_to_lsp_range(uri, content, start as usize, end as usize)
        };
        for &(kind, key, start, end) in &key_spans {
            // An ability is judged against the model the check names, so
            // it reports which model rather than the shared
            // "Unknown <kind>: '<key>'" message the others share.
            if kind == CheckedStringKind::GateAbility {
                if !declared.gate_abilities.is_empty()
                    && let Some(message) = self.gate_ability_problem(
                        ctx,
                        uri,
                        content,
                        key,
                        start,
                        &declared.gate_abilities,
                    )
                    && let Some(range) = range(start, end)
                {
                    out.push(helpers::make_diagnostic(
                        range,
                        DiagnosticSeverity::Warning,
                        "invalid_laravel_ability",
                        message,
                    ));
                }
                continue;
            }
            if let Some((false, label, code)) = declared.judge(kind, key)
                && let Some(range) = range(start, end)
            {
                // A Yii view is keyed by the whole path it names, which
                // reads best from the workspace root, and a route
                // argument's key names its handler too.
                let shown = match kind {
                    CheckedStringKind::YiiView => {
                        std::borrow::Cow::Owned(self.yii_view_display(key))
                    }
                    CheckedStringKind::YiiRouteArgument => std::borrow::Cow::Borrowed(
                        crate::yii::routes::split_route_argument_key(key).1,
                    ),
                    _ => std::borrow::Cow::Borrowed(key),
                };
                out.push(helpers::make_diagnostic(
                    range,
                    DiagnosticSeverity::Warning,
                    code,
                    format!("Unknown {}: '{}'", label, shown),
                ));
            }
        }
    }

    /// Judge one authorization ability, returning the diagnostic message when
    /// it is wrong and `None` when it checks out.
    ///
    /// A check that names a model (`$user->can('update', $post)`,
    /// `Gate::allows('update', Post::class)`) is judged against *that model's*
    /// policy, so a real ability used on the wrong model is caught and named
    /// as such.  A `Gate::define()` registration applies to any subject, so it
    /// satisfies a model-bound check too.  When the model cannot be resolved —
    /// or the call names none — the ability only has to exist somewhere.
    fn gate_ability_problem(
        &self,
        ctx: &helpers::FileDiagnosticContext,
        uri: &str,
        content: &str,
        ability: &str,
        span_start: u32,
        known_abilities: &[String],
    ) -> Option<String> {
        let is_defined = self.laravel.gates.read().definition(ability).is_some();
        if is_defined {
            return None;
        }

        if let Some(model_fqn) = self.gate_subject_model(ctx, uri, content, span_start)
            && let Some((policy, abilities)) =
                crate::virtual_members::laravel::model_policy_abilities(self, &model_fqn)
        {
            if abilities
                .iter()
                .any(|name| name.eq_ignore_ascii_case(ability))
            {
                return None;
            }
            return Some(format!(
                "Ability '{}' is not defined for '{}' (policy {})",
                ability,
                model_fqn,
                policy.fqn()
            ));
        }

        if known_abilities
            .binary_search_by(|known| known.as_str().cmp(ability))
            .is_ok()
        {
            return None;
        }
        Some(format!("Unknown ability: '{}'", ability))
    }

    /// The FQN of the model a gate check named, when the symbol map recorded
    /// one and it resolves to a class.
    fn gate_subject_model(
        &self,
        pass: &helpers::FileDiagnosticContext,
        uri: &str,
        content: &str,
        span_start: u32,
    ) -> Option<String> {
        // A Blade file's symbol map is built from the preprocessed virtual
        // PHP, so every offset in it — including the subject's — indexes that
        // text rather than the template the caller handed us.
        let virtual_php = self.blade_virtual_php_arc(uri);
        let content = virtual_php.as_ref().map_or(content, |php| php.as_str());

        let (subject_text, is_static) = {
            let map = &pass.symbol_map;
            // The subject is stored as a range into the text the map was
            // built from, so a map built from older text would slice the
            // wrong bytes (or none at all).
            let source = map.source(content)?;
            let subject = map.gate_subject(span_start)?;
            (
                subject.subject_text.as_str(source).to_string(),
                subject.is_static,
            )
        };

        // A file with several `namespace` blocks resolves the subject in
        // the one it is written in.
        let narrowed;
        let ctx = if pass.file.namespace_spans.is_some() {
            narrowed = pass.file.at(span_start);
            &narrowed
        } else {
            &pass.file
        };
        let class_loader = self.class_loader(ctx);
        let resolved_class_loader = self.resolved_class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let resolution_ctx = crate::type_engine::subject_resolution::SubjectResolutionCtx {
            local_classes: &ctx.classes,
            use_map: &ctx.use_map,
            namespace: &ctx.namespace,
            content,
            class_loader: &resolved_class_loader,
            source_class_loader: &class_loader,
            backend: Some(self),
            function_loader: &function_loader,
        };
        let name = crate::type_engine::subject_resolution::resolve_subject_type(
            &subject_text,
            is_static,
            span_start,
            &resolution_ctx,
        )?
        .top_level_class_names()
        .into_iter()
        .next()?;
        Some(resolved_class_loader(Fqn::new(&name))?.fqn().to_string())
    }

    /// Whether `literal` names no template this project ships, as a
    /// `view-string` argument demands it does.
    ///
    /// `false` covers "no" and "cannot tell" alike, because only a name
    /// we can prove wrong is worth reporting:
    ///
    /// * An empty literal is a string being typed, not a claim.
    /// * A project the view scan found no templates in tells us nothing
    ///   about any name — it may not be a Laravel project at all, or its
    ///   roots may be somewhere the scan does not reach.
    /// * A `package::name` view lives in a directory a provider
    ///   registers. When no provider registered that namespace, the
    ///   package's templates are simply not enumerable here, so the name
    ///   is unjudgeable rather than wrong.
    pub(crate) fn view_string_literal_is_unknown(&self, literal: &str) -> bool {
        if literal.is_empty() {
            return false;
        }
        let known = self.cached_view_names();
        if known.is_empty() {
            return false;
        }
        let name = crate::virtual_members::laravel::canonical_view_name(literal);
        if let Some((namespace, _)) = name.split_once("::")
            && !self
                .laravel
                .provider_resources
                .read()
                .view_dirs
                .iter()
                .any(|dir| dir.namespace == namespace)
        {
            return false;
        }
        known.binary_search_by(|k| k.as_str().cmp(&name)).is_err()
    }
}

/// Whether `key` is `group` itself or a key nested under it.
fn is_key_or_under(key: &str, group: &str) -> bool {
    key.strip_prefix(group)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Whether the sorted `keys` hold `key` itself or a key nested under it
/// (`app` for `app.name`), which is what makes a partial prefix such as
/// `config('app')` valid.
fn names_key_or_group(keys: &[String], key: &str) -> bool {
    if keys.binary_search_by(|k| k.as_str().cmp(key)).is_ok() {
        return true;
    }
    let group = format!("{key}.");
    let first_under = keys.partition_point(|k| k.as_str() < group.as_str());
    keys.get(first_under).is_some_and(|k| k.starts_with(&group))
}

#[cfg(test)]
mod tests {
    #[test]
    fn laravel_string_key_diagnostics_ignore_an_unindexed_uri() {
        let backend = crate::Backend::new_test();
        let mut out = Vec::new();

        let uri = "file:///closed.php";
        backend.collect_invalid_laravel_string_key_diagnostics(
            &super::helpers::FileDiagnosticContext::gather(&backend, uri),
            uri,
            "<?php config('app.name');",
            &mut out,
        );

        assert!(out.is_empty());
    }

    /// Regression test: `collect_invalid_laravel_string_key_diagnostics`
    /// must not hold the file-state read lock while calling enumeration
    /// functions that reach `ensure_workspace_indexed()` →
    /// `parse_files_parallel()` → `update_ast()` → `files.write()`.
    ///
    /// Before the fix, this deadlocked because the read lock was held
    /// for the entire function body.  The fix extracts the needed spans
    /// into an owned `Vec` and drops the lock before enumerating keys.
    ///
    /// To trigger the deadlock path, we create a workspace with an
    /// unindexed PHP file so `ensure_workspace_indexed` must parse it
    /// (acquiring a write lock).  A 5-second timeout catches the
    /// deadlock as a test failure instead of an infinite hang.
    #[test]
    fn laravel_string_key_diagnostics_no_deadlock() {
        // Set up a temp workspace with an unindexed PHP file so that
        // ensure_workspace_indexed() will call parse_files_parallel()
        // which needs a write lock on symbol_maps.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let tmp = dir.path().to_path_buf();
        let unindexed_file = tmp.join("Unindexed.php");
        std::fs::write(&unindexed_file, "<?php\nclass Unindexed {}\n").unwrap();

        let backend = crate::Backend::new_test_with_workspace(tmp.clone(), vec![]);

        // Register the unindexed file in fqn_uri_index so
        // ensure_workspace_indexed Phase 1 will try to parse it.
        let unindexed_uri = format!("file://{}", unindexed_file.to_str().unwrap());
        backend
            .symbols
            .fqn_uri_index
            .write()
            .insert("Unindexed".to_string(), unindexed_uri);

        // Parse a file with Laravel string key spans.
        let uri = "file:///app/Http/test.php";
        let php = "<?php\nconfig('app.name');\nroute('home');\n";
        backend.update_ast(uri, php);

        // Run the diagnostics in a thread with a timeout so a deadlock
        // is caught as a failure rather than an infinite hang.
        let (tx, rx) = std::sync::mpsc::channel();
        let backend = std::sync::Arc::new(backend);
        let bc = std::sync::Arc::clone(&backend);
        std::thread::spawn(move || {
            let mut out = Vec::new();
            bc.collect_slow_diagnostics(uri, php, &mut out);
            let _ = tx.send(out);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(_diags) => { /* success — no deadlock */ }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!(
                    "collect_slow_diagnostics deadlocked: the file-state read lock \
                     was likely held while enumeration functions tried to write"
                );
            }
            Err(e) => panic!("collect_slow_diagnostics failed: {:?}", e),
        }
    }
}
