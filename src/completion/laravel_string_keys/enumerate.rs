//! Enumerating the keys a project declares: routes, config, views,
//! translations, and gate abilities.
//!
//! Each enumeration walks the workspace from disk, so the results are
//! memoized in the backend's string-key cache and rebuilt only when a file
//! that feeds them changes.

use crate::Backend;

impl Backend {
    /// Enumerate all config keys the merged configuration holds: the
    /// project's `config/` files with the package and framework defaults
    /// merged beneath them, the way Laravel merges them.
    fn enumerate_all_config_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        for (prefix, tree) in self.cached_config_trees().iter() {
            tree.collect_keys(prefix, &mut keys);
        }
        keys.sort();
        keys.dedup();
        keys
    }

    /// Read one slot of [`LaravelStringKeyCache`], building it under
    /// `build_lock` when empty.
    ///
    /// The build is guarded rather than raced because every enumeration
    /// walks the workspace from disk: the parallel diagnostic pass
    /// otherwise has all N workers miss the same empty slot at once and
    /// each repeat the identical walk. Waiters re-check the slot after
    /// acquiring the guard, so exactly one walk happens per
    /// invalidation.
    pub(crate) fn cached_laravel_enumeration<T: Clone>(
        &self,
        build_lock: &parking_lot::Mutex<()>,
        read: impl Fn(&crate::LaravelStringKeyCache) -> Option<T>,
        store: impl Fn(&mut crate::LaravelStringKeyCache, T),
        build: impl FnOnce() -> T,
    ) -> T {
        if let Some(cached) = read(&self.laravel_string_key_cache.read()) {
            return cached;
        }
        let _build_guard = build_lock.lock();
        if let Some(cached) = read(&self.laravel_string_key_cache.read()) {
            return cached;
        }
        let value = build();
        store(&mut self.laravel_string_key_cache.write(), value.clone());
        value
    }

    /// Every named route in the project, with the URI it was registered with,
    /// plus group prefixes whose full set of children is unknowable.
    pub(crate) fn cached_routes(
        &self,
    ) -> std::sync::Arc<crate::virtual_members::laravel::RouteDiscovery> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.routes,
            |cache| cache.routes.clone(),
            |cache, routes| cache.routes = Some(routes),
            || std::sync::Arc::new(crate::virtual_members::laravel::enumerate_all_routes(self)),
        )
    }

    /// Every named route's name, sorted.
    pub(crate) fn cached_route_names(&self) -> std::sync::Arc<[String]> {
        std::sync::Arc::clone(&self.cached_routes().names)
    }

    /// Every config key `config/` declares, sorted.
    pub(crate) fn cached_config_keys(&self) -> std::sync::Arc<[String]> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.config_keys,
            |cache| cache.config_keys.clone(),
            |cache, keys| cache.config_keys = Some(keys),
            || self.enumerate_all_config_keys().into(),
        )
    }

    /// Every Blade view name the project ships, sorted.
    pub(crate) fn cached_view_names(&self) -> std::sync::Arc<[String]> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.view_names,
            |cache| cache.view_names.clone(),
            |cache, names| cache.view_names = Some(names),
            || self.blade_view_names().into(),
        )
    }

    /// Every authorization ability the project defines, from `Gate::define()`
    /// registrations and policy class methods, sorted.
    pub(crate) fn cached_gate_abilities(&self) -> std::sync::Arc<[String]> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.gate_abilities,
            |cache| cache.gate_abilities.clone(),
            |cache, names| cache.gate_abilities = Some(names),
            || crate::virtual_members::laravel::enumerate_gate_abilities(self).into(),
        )
    }

    /// Every translation key, sorted and shared with the translation catalog.
    pub(crate) fn cached_trans_keys(&self) -> std::sync::Arc<[String]> {
        std::sync::Arc::clone(&self.cached_translations().keys)
    }
}
