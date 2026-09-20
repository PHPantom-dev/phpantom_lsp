//! Enumerating the keys a project declares: routes, config, views,
//! translations, and gate abilities.
//!
//! Each enumeration walks the workspace from disk, so the results are
//! memoized in the backend's string-key cache and rebuilt only when a file
//! that feeds them changes.

use crate::Backend;

impl Backend {
    /// Enumerate all config keys by scanning `config/` files and
    /// package config files discovered from service providers.
    fn enumerate_all_config_keys(&self) -> Vec<String> {
        use crate::virtual_members::laravel::{
            collect_laravel_config_declarations, laravel_config_prefix_from_uri,
        };

        let snapshot = self.user_file_symbol_maps();
        let mut keys = Vec::new();

        for (file_uri, _) in &snapshot {
            let Some(prefix) = laravel_config_prefix_from_uri(file_uri) else {
                continue;
            };
            let Some(content) = self.get_file_content(file_uri) else {
                continue;
            };
            let decls = collect_laravel_config_declarations(&content, &prefix);
            for d in decls {
                keys.push(d.key);
            }
        }

        for res in &self.laravel_provider_resources.read().config_files {
            if let Ok(content) = std::fs::read_to_string(&res.path) {
                let decls = collect_laravel_config_declarations(&content, &res.namespace);
                for d in decls {
                    keys.push(d.key);
                }
            }
        }

        if let Some(root) = self.workspace.workspace_root.read().clone() {
            let framework_config = root.join("vendor/laravel/framework/config");
            if framework_config.is_dir()
                && let Ok(entries) = std::fs::read_dir(&framework_config)
            {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.extension().is_some_and(|e| e == "php") {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let prefix = stem.to_string();
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let decls = collect_laravel_config_declarations(&content, &prefix);
                        for d in decls {
                            keys.push(d.key);
                        }
                    }
                }
            }
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

    pub(crate) fn cached_route_names(&self) -> Vec<String> {
        self.cached_routes()
            .routes
            .iter()
            .map(|route| route.name.clone())
            .collect()
    }

    pub(crate) fn cached_config_keys(&self) -> Vec<String> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.config_keys,
            |cache| cache.config_keys.clone(),
            |cache, keys| cache.config_keys = Some(keys),
            || self.enumerate_all_config_keys(),
        )
    }

    pub(crate) fn cached_view_names(&self) -> Vec<String> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.view_names,
            |cache| cache.view_names.clone(),
            |cache, names| cache.view_names = Some(names),
            || self.blade_view_names(),
        )
    }

    /// Every authorization ability the project defines, from `Gate::define()`
    /// registrations and policy class methods.
    pub(crate) fn cached_gate_abilities(&self) -> Vec<String> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.gate_abilities,
            |cache| cache.gate_abilities.clone(),
            |cache, names| cache.gate_abilities = Some(names),
            || crate::virtual_members::laravel::enumerate_gate_abilities(self),
        )
    }

    pub(crate) fn cached_trans_keys(&self) -> Vec<String> {
        self.cached_translations().entries.keys().cloned().collect()
    }
}
