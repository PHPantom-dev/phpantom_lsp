//! The Laravel indexes and caches a `Backend` keeps, grouped so the
//! framework's state sits apart from the language server's own.

use std::collections::{HashMap, HashSet};

use parking_lot::RwLock;

use super::{LaravelStringKeyBuildLocks, LaravelStringKeyCache};
use crate::backend::GatedIndex;
use crate::php_type::PhpType;
use crate::virtual_members::laravel;

/// What the server knows about the project's Laravel side.  Empty for a
/// project that does not use Laravel.
pub(crate) struct LaravelState {
    /// Memoized authenticated-user model type, derived from `config/auth.php`.
    ///
    /// Keyed by guard name (an empty string denotes the default guard).
    /// Populated lazily when an auth-user access is resolved and cleared
    /// whenever files are re-parsed, so edits to `config/auth.php` take
    /// effect without a restart.
    pub(crate) auth_user_type_cache: RwLock<HashMap<String, Option<PhpType>>>,
    /// Memoized concrete type every configured filesystem disk resolves to,
    /// derived from `config/filesystems.php` and
    /// [`storage_drivers`](Self::storage_drivers).
    ///
    /// `Storage`/`FilesystemManager`'s `drive()`/`disk()`/`cloud()`/`build()`
    /// declare only the `Filesystem`/`Cloud` contract; this is what they are
    /// refined to. The outer option is `None` until first computed, the inner
    /// one when at least one disk could not be classified (a dynamic driver
    /// name, or a custom driver with no discoverable registration). Cleared
    /// when a `config/` file or a `Storage::extend()` registration changes, so
    /// an edit takes effect without a restart.
    pub(crate) storage_disk_type_cache: RwLock<Option<Option<PhpType>>>,
    /// `Storage::extend('driver', closure)` registrations discovered from
    /// project and vendor service-provider source, keyed by driver name.
    ///
    /// Built alongside [`macros`](Self::macros) (both come
    /// from the same provider scan) and refreshed when a contributing file
    /// changes. Empty for non-Laravel projects.
    pub(crate) storage_drivers: RwLock<laravel::LaravelStorageDriverIndex>,
    /// Memoized Laravel alias tables, parsed from the installed framework
    /// source (`registerCoreContainerAliases()`, `Facade::defaultAliases()`)
    /// and the project's `config/app.php`.
    ///
    /// `None` means "not yet computed"; an inner empty map means "computed and
    /// this project has no such aliases" (e.g. a non-Laravel project). Cleared
    /// whenever files are re-parsed so edits to `config/app.php` take effect
    /// without a restart.
    ///
    /// The same slot is shared into [`resolved_class_cache`](crate::BackendState::resolved_class_cache)
    /// so the facade virtual member provider, which sees the cache but never
    /// the `Backend`, can map a container-binding accessor to its class.
    pub(crate) aliases: laravel::LaravelAliasSlot,
    /// Laravel `Target::macro('name', closure)` registrations discovered from
    /// project source, keyed by the FQN of the class each macro attaches to.
    ///
    /// Built during `initialized` for Laravel projects and refreshed when a
    /// contributing file changes.  Consulted when a class is loaded so that
    /// macro methods appear in completion, hover, and signature help.  Empty
    /// for non-Laravel projects.
    pub(crate) macros: GatedIndex<laravel::LaravelMacroIndex>,
    /// Reverse index mapping a related-model FQN to the pivot accessors exposed
    /// when it is reached through a many-to-many relationship.
    /// Built lazily (and rebuilt when a pivot-bearing file changes) and
    /// consulted at class load; see [`virtual_members::laravel::pivots`].
    ///
    /// Starts out stale, so the first class load builds it.
    pub(crate) pivots: GatedIndex<laravel::LaravelPivotIndex>,
    /// Index of Artisan console commands (project + vendor) keyed by command
    /// name.  Built during `initialized` for Laravel projects and refreshed
    /// when a command file changes.  Powers command-name completion,
    /// go-to-definition, hover, and unknown-name diagnostics for
    /// `Artisan::call('app:sync')` and friends.  Empty for non-Laravel
    /// projects.  See [`virtual_members::laravel::commands`].
    pub(crate) commands: GatedIndex<laravel::LaravelCommandIndex>,
    /// Eloquent morph map (`Relation::morphMap()` /
    /// `Relation::enforceMorphMap()`) recovered from service-provider source,
    /// keyed by morph alias.  Built during `initialized` for Laravel projects
    /// and refreshed when a registering file changes.  Powers
    /// go-to-definition, hover, and find-references on the alias strings that
    /// appear in `*_type` columns and morph query arguments.  Empty for
    /// non-Laravel projects.  See [`virtual_members::laravel::morph_map`].
    pub(crate) morph_map: RwLock<laravel::LaravelMorphMapIndex>,
    /// Authorization gate registrations (`Gate::define()` abilities and the
    /// model → policy map) recovered from service-provider source.  Built
    /// during `initialized` for Laravel projects and refreshed when a
    /// registering file changes.  Powers completion, hover, go-to-definition,
    /// and the unknown-ability diagnostic for the strings `Gate::allows()`,
    /// `$user->can()`, `$this->authorize()`, and `@can` check.  Empty for
    /// non-Laravel projects.  See [`virtual_members::laravel::gates`].
    pub(crate) gates: RwLock<laravel::LaravelGateIndex>,
    /// Config keys the project declares at runtime rather than in a
    /// `config/` file (`Config::set()`, the array form of the `config()`
    /// helper, `Storage::fake()`), keyed by the file each write is written
    /// in so an edit that removes one takes the key with it.  A test that
    /// configures a disk in `setUp()` before exercising it is the common
    /// shape.  Empty for non-Laravel projects.
    pub(crate) runtime_config_keys: RwLock<HashMap<String, Vec<String>>>,
    /// Laravel macro seed files (service providers plus the app's provider
    /// registration files), mapped to the class references each contributed
    /// at the last macro-index build.  An edit that changes a seed's
    /// references triggers a full index rebuild; every other edit takes the
    /// cheap single-file refresh path.
    pub(crate) macro_seeds: RwLock<HashMap<String, Vec<String>>>,
    /// URIs of the mixin-class files that `Macroable::mixin()` registrations
    /// pulled macros from at the last macro-index build.  A mixin class's
    /// methods live in a different file than the `::mixin(...)` call, and that
    /// file contains no `macro(`/`mixin(` token of its own, so the single-file
    /// refresh cannot see it as a contributor.  An edit to one of these files
    /// (e.g. adding a helper method) therefore triggers a full index rebuild.
    pub(crate) macro_mixin_uris: RwLock<HashSet<String>>,
    /// Concrete date class selected by the project's `Date::use()` call.
    ///
    /// The outer option is `None` until startup discovery completes; the inner
    /// option is `None` when discovery found no project override.
    pub(crate) date_class: RwLock<Option<Option<String>>>,
    /// URIs whose edits can change the configured date class: every registered
    /// service provider scanned by [`build_laravel_date_class`], plus the app's
    /// provider-registration files (which decide *which* providers are
    /// registered).  Populated by that scan and consulted by the single-file
    /// refresh so a `Date::use()` added, changed, or removed in one of these
    /// files re-runs the full scan, while an edit to any other file is ignored.
    ///
    /// [`build_laravel_date_class`]: crate::Backend::build_laravel_date_class
    pub(crate) date_seed_uris: RwLock<HashSet<String>>,
    /// What the project's registered service providers register: container
    /// bindings, package config files, view and translation directories, route
    /// files, and Blade component namespaces, merged across every provider.
    ///
    /// Built by [`build_provider_resources`](crate::Backend::build_provider_resources)
    /// and republished by
    /// [`refresh_laravel_provider_resources`](crate::Backend::refresh_laravel_provider_resources)
    /// when a provider is edited.
    pub(crate) provider_resources: RwLock<laravel::ProviderResources>,
    /// The per-provider scans the merged table above was built from, so an
    /// edit to one provider rebuilds the merge without re-reading the rest.
    pub(crate) provider_scans: RwLock<laravel::ProviderScans>,
    /// Cached Laravel string key enumerations (route names, config keys,
    /// view names, translation keys).  `None` = not yet computed.
    /// Invalidated when a file in `routes/`, `config/`, `resources/views/`,
    /// or `lang/` is updated.
    pub(crate) string_key_cache: RwLock<LaravelStringKeyCache>,
    /// Compute-once guards for `laravel_string_key_cache`; see
    /// [`LaravelStringKeyBuildLocks`].
    pub(crate) string_key_build_locks: LaravelStringKeyBuildLocks,
    pub(crate) schema_index: RwLock<laravel::database_schema::SchemaIndex>,
    /// Lazily confirmed morph-column aliases, invalidated when source or
    /// the model metadata used to recognize their columns changes.
    pub(crate) morph_column_spans_cache:
        RwLock<HashMap<String, laravel::typed_morph_columns::MorphColumnSpans>>,
}

impl LaravelState {
    /// Empty indexes around the alias slot the resolved-class cache shares.
    pub(crate) fn new(aliases: laravel::LaravelAliasSlot) -> Self {
        Self {
            auth_user_type_cache: RwLock::new(HashMap::new()),
            storage_disk_type_cache: RwLock::new(None),
            storage_drivers: RwLock::default(),
            aliases,
            macros: GatedIndex::default(),
            pivots: GatedIndex::stale(laravel::LaravelPivotIndex::default()),
            commands: GatedIndex::default(),
            morph_map: RwLock::default(),
            gates: RwLock::default(),
            runtime_config_keys: RwLock::default(),
            macro_seeds: RwLock::default(),
            macro_mixin_uris: RwLock::default(),
            date_class: RwLock::new(None),
            date_seed_uris: RwLock::default(),
            provider_resources: RwLock::default(),
            provider_scans: RwLock::default(),
            string_key_cache: RwLock::default(),
            string_key_build_locks: LaravelStringKeyBuildLocks::default(),
            schema_index: RwLock::default(),
            morph_column_spans_cache: RwLock::default(),
        }
    }
}
