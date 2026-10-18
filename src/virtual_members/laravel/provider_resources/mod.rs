//! The resources Laravel service providers register: config files and the
//! config keys written at runtime, view and translation directories, route
//! files, container bindings, Blade component namespaces and directives,
//! shared view data and composers, and Folio mounts.
//!
//! [`extract_provider_resources`] scans one provider file.  This module
//! holds what a scan records, and how the scans of every registered provider
//! merge into the one table the rest of the server reads.

mod bindings;
mod extract;
mod package_tools;
mod paths;
#[cfg(test)]
mod tests;
mod views;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::PathBuf;
use std::sync::Arc;

use super::view_data::{SharedViewVar, ViewComposer};
use crate::ci_map::CiSet;

pub(crate) use extract::extract_provider_resources;
pub(crate) use paths::{PathContext, resolve_path_arg};

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
    /// Config keys a provider writes at runtime rather than ships in a file,
    /// in dot notation: `laravel/sanctum` adds its guard with
    /// `config(['auth.guards.sanctum' => …])` from `register()`.
    pub config_writes: Vec<String>,
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
    /// here (`crate::blade::directives::BladeDirectives`).
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
        self.config_writes.extend(other.config_writes);
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

    /// Drop one provider's scan, for a file deleted from disk.  Its FQN stays
    /// registered, so a file that declares the provider again rejoins the
    /// table the moment it parses.  Returns whether anything was dropped.
    pub fn remove(&mut self, uri: &str) -> bool {
        let before = self.scans.len();
        self.scans.retain(|scan| scan.uri != uri);
        self.scans.len() != before
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
