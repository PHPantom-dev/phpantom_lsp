use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::const_eval::{ClassContext, Scope, const_string};
use super::view_data::{SharedViewVar, ViewComposer, ViewDataRegistration, view_data_registration};
use crate::atom::bytes_to_str;
use crate::ci_map::CiSet;
use crate::names::OwnedResolvedNames;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderResource {
    pub path: PathBuf,
    pub namespace: String,
}

/// How a service provider came to be registered, which decides whose binding
/// the container ends up with when two providers bind the same string key.
///
/// The order mirrors `Application::registerConfiguredProviders()`, which
/// registers the `Illuminate\*` entries of the configured list first, then the
/// providers vendor packages auto-discover, then everything else the
/// application lists.  Each registration replaces the one before it, so a
/// higher variant wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) enum ProviderOrigin {
    /// An `Illuminate\*` provider: a framework default.
    #[default]
    Framework,
    /// Auto-discovered from a vendor package's `extra.laravel.providers`.
    Package,
    /// Listed by the application in `bootstrap/providers.php` or the
    /// `providers` key of `config/app.php`.
    Application,
}

/// The provider a scan is reading, as far as binding precedence goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProviderIdentity {
    /// The provider class's FQN.
    pub fqn: String,
    /// The provider classes it extends.  Subclassing a provider and re-binding
    /// one of its keys is how a replacement is written, so the parent must not
    /// win the key back merely by being scanned later.
    pub ancestors: Vec<String>,
    pub origin: ProviderOrigin,
}

impl ProviderIdentity {
    /// Whether a binding this provider made survives one `other` makes for the
    /// same key.
    ///
    /// Providers are scanned in registration order and each registration
    /// replaces the one before it, so the tie goes to `other` unless this
    /// provider is registered later (a higher [`ProviderOrigin`]) or extends
    /// it.
    fn outranks(&self, other: &ProviderIdentity) -> bool {
        match self.origin.cmp(&other.origin) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => self.ancestors.iter().any(|parent| parent == &other.fqn),
        }
    }
}

/// The concrete class behind a container key, with the provider that put it
/// there so a provider scanned later is weighed against it rather than
/// overwriting it outright.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    pub class: String,
    /// Where the registration that produced this binding is written, so the
    /// key can be navigated to and described.
    pub site: BindingSite,
    provider: Arc<ProviderIdentity>,
}

/// The registration a container key came from: the provider file it is
/// written in, and the byte offset the key itself starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BindingSite {
    /// The service provider file holding the registration.
    pub path: PathBuf,
    /// Byte offset of the key — inside the quotes of a literal, or the start
    /// of whatever expression names it (a class constant, a static property).
    pub offset: u32,
}

/// An `alias('other-key', 'key')` entry: the key it stands for, and the
/// provider that named it, so precedence applies to aliases too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Alias {
    target: String,
    site: BindingSite,
    provider: Arc<ProviderIdentity>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProviderResources {
    pub config_files: Vec<ProviderResource>,
    pub view_dirs: Vec<ProviderResource>,
    pub trans_dirs: Vec<ProviderResource>,
    pub route_files: Vec<PathBuf>,
    /// Container binding key → the class bound to it, for the bindings a
    /// provider makes under a *string* abstract
    /// (`$this->app->singleton('sentry', fn () => new HubAdapter())`).  A
    /// binding keyed by `Contract::class` needs no table: the written name
    /// already resolves.
    pub bindings: HashMap<String, Binding>,
    /// `alias('other-key', 'key')` entries, as key → the key they stand for.
    /// These name a binding by another *string* key rather than by a class,
    /// so they are folded into `bindings` by [`Self::resolve_aliases`] once
    /// every provider has been scanned: the key an alias points at may well
    /// be bound by a provider that comes later.
    pub aliases: HashMap<String, Alias>,
    /// `Blade::componentNamespace('Nightshade\Views\Components', 'nightshade')`
    /// entries, as (tag prefix, class namespace).  A view addressed under the
    /// prefix (`nightshade::calendar`) is backed by a component class in that
    /// namespace, whose members its template reads.
    pub class_component_namespaces: Vec<(String, String)>,
    /// `Blade::anonymousComponentNamespace('components', 'webshop')` entries,
    /// as (tag prefix, view directory in dot notation).  A tag written under
    /// the prefix (`<x-webshop::pages.boxes>`) names a plain view in that
    /// directory (`components.pages.boxes`) rather than a component class.
    pub anonymous_component_namespaces: Vec<(String, String)>,
    /// `Blade::anonymousComponentPath(resource_path('views/components'),
    /// 'webshop')` entries, as (tag prefix, directory on disk).  The same
    /// mechanism keyed by a directory; which view prefix it stands for is
    /// only known once it is matched against the configured view roots,
    /// which this scan cannot see.  An empty prefix is the prefix-less
    /// registration, whose templates every un-namespaced tag can address.
    pub anonymous_component_paths: Vec<(String, PathBuf)>,
    /// `Blade::directive('datetime', …)` and `Blade::if('admin', …)`
    /// registrations, in registration order.  These name directives the
    /// preprocessor would otherwise mask as comments, and the four members
    /// of a `Blade::if()` family are expanded from the single name recorded
    /// here (`crate::blade::directives::CustomDirectives`).
    pub custom_directives: Vec<crate::blade::directives::CustomDirective>,
    /// `View::share('key', $value)` registrations, which put a variable in
    /// every template's scope.
    pub shared_view_vars: Vec<SharedViewVar>,
    /// `View::composer(…)` registrations, which put variables in the scope of
    /// the views each one targets.
    pub view_composers: Vec<ViewComposer>,
    /// A provider rebound `translator` or `translation.loader` to something
    /// other than Laravel's own file-based pair, so the strings come from a
    /// source we cannot enumerate (a database table, say) and the set of
    /// valid translation keys is unknowable.
    pub custom_translation_loader: bool,
    /// `Folio::path(...)`/`Folio::route(...)` mount registrations, typically
    /// made from a service provider's `boot()` for a second page directory
    /// (the primary mount usually lives in `bootstrap/app.php`'s
    /// `withRouting(pages: ...)`, which this scan does not reach since it is
    /// not a provider — see `super::folio::discover_folio_mounts`).
    pub folio_mounts: Vec<super::folio::FolioMount>,
}

impl ProviderResources {
    pub fn merge(&mut self, other: ProviderResources) {
        self.config_files.extend(other.config_files);
        self.view_dirs.extend(other.view_dirs);
        self.trans_dirs.extend(other.trans_dirs);
        self.route_files.extend(other.route_files);
        self.class_component_namespaces
            .extend(other.class_component_namespaces);
        self.anonymous_component_namespaces
            .extend(other.anonymous_component_namespaces);
        self.anonymous_component_paths
            .extend(other.anonymous_component_paths);
        self.custom_directives.extend(other.custom_directives);
        self.shared_view_vars.extend(other.shared_view_vars);
        self.view_composers.extend(other.view_composers);
        self.folio_mounts.extend(other.folio_mounts);
        for (key, binding) in other.bindings {
            self.record_binding(key, binding);
        }
        for (key, alias) in other.aliases {
            match self.aliases.entry(key) {
                Entry::Occupied(mut slot) => {
                    if !slot.get().provider.outranks(&alias.provider) {
                        slot.insert(alias);
                    }
                }
                Entry::Vacant(slot) => {
                    slot.insert(alias);
                }
            }
        }
        self.custom_translation_loader |= other.custom_translation_loader;
    }

    /// Bind `key`, unless a provider that outranks this one already claimed it.
    ///
    /// Two providers binding the same key is the normal way an application
    /// swaps a framework or package implementation out, so the key has to end
    /// up with the class the container would hold once every provider has
    /// registered.
    fn record_binding(&mut self, key: String, binding: Binding) {
        match self.bindings.entry(key) {
            Entry::Occupied(mut slot) => {
                if !slot.get().provider.outranks(&binding.provider) {
                    slot.insert(binding);
                }
            }
            Entry::Vacant(slot) => {
                slot.insert(binding);
            }
        }
    }

    /// Give every alias the concrete class of the key it stands for.
    ///
    /// Laravel resolves an alias by following it until it reaches a key that
    /// is not itself aliased, so a chain (`'a'` → `'b'` → a bound class) is
    /// followed here too.  The walk is bounded by the number of aliases, which
    /// leaves a cycle unresolved instead of looping forever.
    pub fn resolve_aliases(&mut self) {
        let limit = self.aliases.len();
        let resolved: Vec<(String, Binding)> = self
            .aliases
            .iter()
            .filter_map(|(key, alias)| {
                let mut target = &alias.target;
                for _ in 0..limit {
                    match self.aliases.get(target) {
                        Some(next) => target = &next.target,
                        None => break,
                    }
                }
                let concrete = self.bindings.get(target)?;
                Some((
                    key.clone(),
                    Binding {
                        class: concrete.class.clone(),
                        // The alias is where *this* key is written, so it is
                        // what its go-to-definition should land on.
                        site: alias.site.clone(),
                        provider: Arc::clone(&alias.provider),
                    },
                ))
            })
            .collect();
        // The container consults its alias table before its bindings, so an
        // alias decides the key it covers, subject to the same precedence as
        // any other registration.
        for (key, binding) in resolved {
            self.record_binding(key, binding);
        }
    }
}

/// One provider file's own registrations, before they are merged with every
/// other provider's.
#[derive(Debug)]
pub(crate) struct ProviderScan {
    /// The provider file the registrations were read from.
    pub uri: String,
    pub identity: Arc<ProviderIdentity>,
    pub resources: ProviderResources,
}

/// The per-provider scans the merged resource table was built from.
///
/// Keeping each provider's own registrations means an edit to one provider
/// re-parses that file alone: the merged table is rebuilt from the cached
/// scans rather than by re-reading every registered provider, which on a real
/// application is hundreds of files.
#[derive(Debug, Default)]
pub(crate) struct ProviderScans {
    /// The scans in registration order.  [`ProviderResources::merge`] settles
    /// a key two providers bind by that order, so it has to be preserved.
    scans: Vec<ProviderScan>,
    /// Every registered provider's FQN, including the ones that had no file to
    /// scan, so a provider written after the list that names it is recognised
    /// the moment its class appears.
    registered: CiSet,
    /// Whether the full scan has run.  Until it has there is no table to keep
    /// coherent, and a single-file refresh would publish a table holding only
    /// the edited provider's registrations.
    built: bool,
}

impl ProviderScans {
    pub fn record(&mut self, scan: ProviderScan) {
        self.scans.push(scan);
    }

    pub fn record_registered(&mut self, fqn: &str) {
        self.registered.insert(fqn);
    }

    pub fn mark_built(&mut self) {
        self.built = true;
    }

    pub fn is_built(&self) -> bool {
        self.built
    }

    /// Whether `fqn` names a registered provider, whether or not its file was
    /// resolvable when the full scan ran.
    pub fn is_registered(&self, fqn: &str) -> bool {
        self.registered.contains(fqn)
    }

    pub fn scan_for(&self, uri: &str) -> Option<&ProviderScan> {
        self.scans.iter().find(|scan| scan.uri == uri)
    }

    /// Replace one provider's registrations, leaving every other scan in
    /// place.  Returns `false` when the provider is no longer part of the
    /// table (a rebuild raced this refresh).
    pub fn replace(
        &mut self,
        uri: &str,
        identity: Arc<ProviderIdentity>,
        resources: ProviderResources,
    ) -> bool {
        let Some(scan) = self.scans.iter_mut().find(|scan| scan.uri == uri) else {
            return false;
        };
        scan.identity = identity;
        scan.resources = resources;
        true
    }

    /// The merged table every consumer reads, with aliases resolved against
    /// the complete set of bindings.
    pub fn merged(&self) -> ProviderResources {
        let mut merged = ProviderResources::default();
        for scan in &self.scans {
            merged.merge(scan.resources.clone());
        }
        // Every provider has been scanned, so an alias can now be pointed at
        // the binding it stands for regardless of which provider made it.
        merged.resolve_aliases();
        merged
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

        // `Blade::componentNamespace('Nightshade\\Views\\Components',
        // 'nightshade')` says which classes back the views a package
        // registers under its own prefix; the two anonymous registrations
        // point a prefix at a directory of class-less component views.
        if let Expression::Call(Call::StaticMethod(sc)) = expr
            && let ClassLikeMemberSelector::Identifier(method) = &sc.method
            && let Expression::Identifier(id) = sc.class
            && id
                .value()
                .rsplit(|&b| b == b'\\')
                .next()
                .is_some_and(|seg| seg.eq_ignore_ascii_case(b"Blade"))
            && {
                let method_lower = method.value.to_ascii_lowercase();
                record_component_registration(
                    &method_lower,
                    &sc.argument_list,
                    content,
                    &scope,
                    &PathContext {
                        file_dir,
                        workspace_root,
                        program,
                    },
                    &mut resources,
                ) || record_directive_registration(
                    &method_lower,
                    &sc.argument_list,
                    content,
                    &scope,
                    &mut resources,
                )
            }
        {
            return ControlFlow::Continue(());
        }

        // `View::share()` / `View::composer()` and the same calls written
        // against the container's view factory.  Checked ahead of the generic
        // method-call handling below, which only looks at `$this->…` and
        // `$this->app->…` receivers.
        if let Some(registration) =
            view_data_registration(expr, content, file_path, &scope, &resolved)
        {
            match registration {
                ViewDataRegistration::Shared(vars) => resources.shared_view_vars.extend(vars),
                ViewDataRegistration::Composers(composers) => {
                    resources.view_composers.extend(composers)
                }
            }
            return ControlFlow::Continue(());
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

        // `$this->app->alias(Concrete::class, 'key')` gives an existing
        // binding another name.  The arguments read the other way round from
        // `bind()`: the key is the second one, and the first names what it
        // stands for.
        if method_lower == b"alias"
            && is_app_container_expr(mc.object)
            && let Some(target) = mc.argument_list.arguments.iter().next()
            && let Some(key_arg) = mc.argument_list.arguments.iter().nth(1)
            && let Some(key) = alias_key(key_arg.value(), content, &scope, &resolved)
        {
            let site = BindingSite {
                path: file_path.to_path_buf(),
                offset: key_offset(key_arg.value()),
            };
            match binding_concrete(Some(target.value()), &resolved) {
                Some(concrete) => {
                    resources.bindings.insert(
                        key,
                        Binding {
                            class: concrete,
                            site,
                            provider: Arc::clone(&provider),
                        },
                    );
                }
                // The target is another string key, whose own binding may not
                // have been scanned yet.
                None => {
                    if let Some(aliased) = const_string(target.value(), content, &scope) {
                        resources.aliases.insert(
                            key,
                            Alias {
                                target: aliased,
                                site,
                                provider: Arc::clone(&provider),
                            },
                        );
                    }
                }
            }
            return ControlFlow::Continue(());
        }

        // `$this->app->singleton('translation.loader', …)` and friends decide
        // where translation lines come from, and the container is reached
        // through `$this->app`, not `$this`, so this is checked ahead of the
        // `$this->…` resource loaders below.
        if BINDING_METHODS.contains(&method_lower.as_slice())
            && is_app_container_expr(mc.object)
            && let Some(key_arg) = mc.argument_list.arguments.iter().next()
            && let Some(key) = const_string(key_arg.value(), content, &scope)
        {
            let factory = mc.argument_list.arguments.iter().nth(1).map(|a| a.value());

            // A translation binding decides where the strings come from, on top
            // of naming a class: anything but Laravel's own file-based pair
            // reads its lines from a source we cannot enumerate.  `extend`
            // decorates whatever is already bound, so even a file-based wrapper
            // adds lines from somewhere else.
            if TRANSLATION_BINDINGS.contains(&key.as_str())
                && (method_lower == b"extend" || !builds_file_translator(factory))
            {
                resources.custom_translation_loader = true;
            }

            // `extend` wraps whatever the key already holds; which class comes
            // out depends on the binding it decorates, so only the calls that
            // *replace* the value tell us the concrete type.
            if method_lower != b"extend"
                && let Some(concrete) = binding_concrete(factory, &resolved)
            {
                resources.bindings.insert(
                    key,
                    Binding {
                        class: concrete,
                        site: BindingSite {
                            path: file_path.to_path_buf(),
                            offset: key_offset(key_arg.value()),
                        },
                        provider: Arc::clone(&provider),
                    },
                );
            }
            return ControlFlow::Continue(());
        }

        // The same registrations written against the compiler instance a
        // deferred callback receives (`$blade->componentNamespace(…)`),
        // which is how a package registers before Blade is resolved.
        if record_component_registration(
            &method_lower,
            &mc.argument_list,
            content,
            &scope,
            &PathContext {
                file_dir,
                workspace_root,
                program,
            },
            &mut resources,
        ) {
            return ControlFlow::Continue(());
        }

        // `$blade->directive(…)` / `$blade->if(…)`, written the same way.
        // Unlike the component-namespace methods above, these two have
        // names an unrelated fluent API could plausibly carry (`->if()`
        // most of all), so the receiver has to actually name Blade's
        // compiler.
        if is_blade_compiler_expr(mc.object, content, &scope, &resolved)
            && record_directive_registration(
                &method_lower,
                &mc.argument_list,
                content,
                &scope,
                &mut resources,
            )
        {
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

    // `public $bindings = ['payments' => StripeGateway::class]` and its
    // `$singletons` twin are applied by `Application::register()` *after* the
    // provider's own `register()` has run, so they overwrite what the walk
    // above found for the same key, exactly as the container would.
    for stmt in program.statements.iter() {
        scan_property_bindings(
            stmt,
            content,
            &scope,
            &resolved,
            file_path,
            &provider,
            &mut resources,
        );
    }

    if registers_routes_inline {
        resources.route_files.push(file_path.to_path_buf());
    } else {
        resources.route_files.extend(grouped_route_files);
    }

    resources.folio_mounts =
        super::folio::scan_folio_mounts_in_program(program, content, file_dir, workspace_root);

    resources
}

/// The two provider properties Laravel reads container registrations out of,
/// as an alternative to writing them in `register()`.
const BINDING_PROPERTIES: [&[u8]; 2] = [b"$bindings", b"$singletons"];

/// Record the string-keyed entries of a provider's `$bindings` /
/// `$singletons` arrays, descending through namespace and block statements to
/// find the class that declares them.
///
/// Only a string key is recorded: an entry keyed by `Contract::class` binds a
/// name that already resolves on its own, and retyping it to the concrete
/// would contradict the contract the application declared.
fn scan_property_bindings(
    stmt: &Statement<'_>,
    content: &str,
    scope: &Scope,
    resolved: &OwnedResolvedNames,
    file_path: &Path,
    provider: &Arc<ProviderIdentity>,
    resources: &mut ProviderResources,
) {
    let members = match stmt {
        Statement::Class(class) => &class.members,
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                scan_property_bindings(
                    inner, content, scope, resolved, file_path, provider, resources,
                );
            }
            return;
        }
        Statement::Block(block) => {
            for inner in block.statements.iter() {
                scan_property_bindings(
                    inner, content, scope, resolved, file_path, provider, resources,
                );
            }
            return;
        }
        _ => return,
    };

    for member in members.iter() {
        let ClassLikeMember::Property(Property::Plain(prop)) = member else {
            continue;
        };
        for item in prop.items.iter() {
            let PropertyItem::Concrete(concrete) = item else {
                continue;
            };
            if !BINDING_PROPERTIES.contains(&concrete.variable.name) {
                continue;
            }
            let Expression::Array(array) = concrete.value else {
                continue;
            };
            for element in array.elements.iter() {
                let ArrayElement::KeyValue(kv) = element else {
                    continue;
                };
                let Some(key) = const_string(kv.key, content, scope) else {
                    continue;
                };
                // An integer key is Laravel's shorthand for binding the class
                // to itself, which needs no table.
                if key.parse::<i64>().is_ok() {
                    continue;
                }
                let Some(class) = binding_concrete(Some(kv.value), resolved) else {
                    continue;
                };
                resources.bindings.insert(
                    key,
                    Binding {
                        class,
                        site: BindingSite {
                            path: file_path.to_path_buf(),
                            offset: key_offset(kv.key),
                        },
                        provider: Arc::clone(provider),
                    },
                );
            }
        }
    }
}

/// Where go-to-definition lands for a registration's key: just inside the
/// quotes of a string literal, or the start of whatever else names it.
fn key_offset(expr: &Expression<'_>) -> u32 {
    match expr {
        Expression::Literal(literal::Literal::String(s)) => s.span.start.offset + 1,
        other => other.span().start.offset,
    }
}

/// What a path argument is resolved against: the referring file's directory
/// and the workspace root, plus the parse a variable's assignment is looked
/// up in.
struct PathContext<'a, 'arena> {
    file_dir: &'a Path,
    workspace_root: &'a Path,
    program: &'a Program<'arena>,
}

/// Record a component-namespace registration, whichever of the three
/// `method` names, and report whether the call was one of them.
///
/// All three are written both on the `Blade` facade and on the compiler
/// instance a deferred callback receives, so both call shapes route here.
fn record_component_registration(
    method: &[u8],
    argument_list: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
    paths: &PathContext<'_, '_>,
    resources: &mut ProviderResources,
) -> bool {
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
                && let Some(path) = resolve_path_arg(
                    path_arg.value(),
                    content,
                    paths.file_dir,
                    paths.workspace_root,
                    paths.program,
                )
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
    method: &[u8],
    argument_list: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
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
        && let Some(name) = const_string(first.value(), content, scope)
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
fn is_blade_compiler_expr(
    expr: &Expression<'_>,
    content: &str,
    scope: &Scope,
    resolved: &OwnedResolvedNames,
) -> bool {
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
            const_string(access.index, content, scope).as_deref() == Some(COMPILER_KEY)
        }
        // `app('blade.compiler')` and `$this->app->make(BladeCompiler::class)`
        Expression::Call(Call::Function(fc)) => {
            names_blade_compiler(&fc.argument_list, content, scope, resolved)
        }
        Expression::Call(Call::Method(mc)) => {
            names_blade_compiler(&mc.argument_list, content, scope, resolved)
        }
        Expression::Parenthesized(inner) => {
            is_blade_compiler_expr(inner.expression, content, scope, resolved)
        }
        _ => false,
    }
}

/// Whether a container lookup's first argument names Blade's compiler,
/// either by its container key or by the compiler class itself.
fn names_blade_compiler(
    argument_list: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
    resolved: &OwnedResolvedNames,
) -> bool {
    let Some(first) = argument_list.arguments.iter().next() else {
        return false;
    };
    let named = const_string(first.value(), content, scope)
        .or_else(|| class_string_fqn(first.value(), resolved));
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

fn is_this_expr(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
    )
}

/// Whether `expr` names the service container: `$this->app` in a provider
/// method, the `$app` a deferred callback receives, or the `app()` helper.
pub(in crate::virtual_members::laravel) fn is_app_container_expr(expr: &Expression<'_>) -> bool {
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
/// (`singleton('foo', fn () => new Foo())` or its closure equivalent).
///
/// A factory that declares a return type is taken at its word, the way a
/// method's declared return type decides what a call to it yields: a factory
/// written `function (): Gateway { return new StripeGateway(); }` binds the
/// contract its author named, not the implementation behind it.  Only an
/// undeclared factory is read from its body, whose first `return` decides the
/// type.  A factory that declares nothing and hands back anything else (a
/// container lookup, a variable, a conditional) yields `None`: guessing there
/// would bind the key to a class the application never resolves.
fn binding_concrete(
    expr: Option<&Expression<'_>>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    let expr = expr?;
    match expr {
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(id) => resolved_class_fqn(id, resolved),
            _ => None,
        },
        Expression::Access(Access::ClassConstant(_)) => class_string_fqn(expr, resolved),
        Expression::ArrowFunction(arrow) => {
            declared_factory_return(arrow.return_type_hint.as_ref(), resolved)
                .or_else(|| binding_concrete(Some(arrow.expression), resolved))
        }
        Expression::Closure(closure) => {
            declared_factory_return(closure.return_type_hint.as_ref(), resolved)
                .or_else(|| closure_binding_concrete(closure, resolved))
        }
        Expression::Parenthesized(inner) => binding_concrete(Some(inner.expression), resolved),
        _ => None,
    }
}

/// The class a factory's own `: Concrete` return type names.
///
/// Nothing but a single class name counts: a union, an intersection, or a
/// built-in type does not name one class the key holds.  A nullable hint
/// (`?Concrete`) still names one, since a container that hands back `null`
/// for the key has nothing else it could be resolved as.
fn declared_factory_return(
    return_type_hint: Option<&FunctionLikeReturnTypeHint<'_>>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    super::macros::resolve_hint_target_fqn(&return_type_hint?.hint, resolved)
}

/// The concrete class a closure factory's `return` hands back.
///
/// A factory often configures the instance before returning it
/// (`$translator = new DatabaseTranslator(…); $translator->setFallback(…);
/// return $translator;`), so a returned variable is followed back to the
/// last value assigned to it above the return.
fn closure_binding_concrete(
    closure: &Closure<'_>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    let mut assignments: Vec<(&[u8], &Expression<'_>)> = Vec::new();
    for stmt in closure.body.statements.iter() {
        match stmt {
            Statement::Return(ret) => {
                if let Some(concrete) = binding_concrete(ret.value, resolved) {
                    return Some(concrete);
                }
                let Some(Expression::Variable(Variable::Direct(target))) = ret.value else {
                    return None;
                };
                return assignments
                    .iter()
                    .rev()
                    .find(|(name, _)| *name == target.name)
                    .and_then(|(_, rhs)| binding_concrete(Some(rhs), resolved));
            }
            Statement::Expression(expr_stmt) => {
                if let Expression::Assignment(assign) = expr_stmt.expression
                    && assign.operator.is_assign()
                    && let Expression::Variable(Variable::Direct(lhs)) = assign.lhs
                {
                    assignments.push((lhs.name, assign.rhs));
                }
            }
            _ => {}
        }
    }
    None
}

/// The container key an `alias()` argument names.
///
/// An alias is keyed either by a string (`alias(HubInterface::class,
/// 'sentry')`) or by a contract's name (`alias('sentry',
/// HubInterface::class)`), and both sides of the call accept both forms.
fn alias_key(
    expr: &Expression<'_>,
    content: &str,
    scope: &Scope,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    const_string(expr, content, scope).or_else(|| class_string_fqn(expr, resolved))
}

/// The FQN an `X::class` expression spells, or `None` for any other
/// expression.
fn class_string_fqn(expr: &Expression<'_>, resolved: &OwnedResolvedNames) -> Option<String> {
    let Expression::Access(Access::ClassConstant(access)) = expr else {
        return None;
    };
    if !matches!(
        &access.constant,
        ClassLikeConstantSelector::Identifier(constant)
            if constant.value.eq_ignore_ascii_case(b"class")
    ) {
        return None;
    }
    match access.class {
        Expression::Identifier(id) => resolved_class_fqn(id, resolved),
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

    // `base_path('app/.../web.php')` resolves relative to the workspace
    // root, `resource_path('views/components')` relative to `resources/`
    // inside it.  Both take an optional argument, and naming the base
    // directory alone is a path in its own right.
    if let Expression::Call(Call::Function(fc)) = expr
        && let Expression::Identifier(id) = fc.function
        && let Some(base) = path_helper_base(id.value())
        && let Some(val) = match fc.argument_list.arguments.iter().next() {
            Some(first_arg) => super::helpers::extract_string_literal(first_arg.value(), content)
                .map(|(val, _, _)| val),
            None => Some(""),
        }
    {
        let mut resolved = workspace_root.to_path_buf();
        for segment in [base, val.trim_start_matches('/')] {
            if !segment.is_empty() {
                resolved.push(segment);
            }
        }
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

/// The workspace-relative directory a Laravel path helper resolves against,
/// or `None` for a function that is not one.
fn path_helper_base(name: &[u8]) -> Option<&'static str> {
    const HELPERS: [(&[u8], &str); 2] = [(b"base_path", ""), (b"resource_path", "resources")];
    let short = name.rsplit(|&b| b == b'\\').next()?;
    HELPERS
        .iter()
        .find(|(helper, _)| short.eq_ignore_ascii_case(helper))
        .map(|(_, base)| *base)
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
}
