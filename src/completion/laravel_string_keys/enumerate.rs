//! Enumerating the keys a project declares: routes, config, views,
//! translations, and gate abilities.
//!
//! Each enumeration walks the workspace from disk, so the results are
//! memoized in the backend's string-key cache and rebuilt only when a file
//! that feeds them changes.

use std::collections::HashMap;

use crate::Backend;

impl Backend {
    /// Enumerate all config keys by scanning `config/` files and
    /// package config files discovered from service providers.
    fn enumerate_all_config_keys(&self) -> Vec<String> {
        use crate::virtual_members::laravel::collect_laravel_config_declarations;

        let mut keys = Vec::new();
        self.for_each_config_source(|prefix, content| {
            keys.extend(
                collect_laravel_config_declarations(content, prefix)
                    .into_iter()
                    .map(|d| d.key),
            );
        });
        keys.sort();
        keys.dedup();
        keys
    }

    /// Enumerate every translation key alongside whether it names a
    /// translation group (a nested array) rather than a scalar string
    /// entry, merging the flag across every locale and file that
    /// declares the key.
    ///
    /// Covers PHP array files (`lang/en/messages.php` → `messages.key`),
    /// JSON translation files (`lang/en.json` → raw key strings), and
    /// package translation directories discovered from service providers
    /// (`namespace::file.key`).
    ///
    /// A key that is a group in *any* locale is recorded as a group even
    /// if another locale happens to declare it as a scalar — the return
    /// type narrowing this feeds is only safe when every locale agrees
    /// the entry is scalar.
    ///
    /// The project's lang files are discovered with a direct disk walk
    /// rather than through `user_file_symbol_maps`, for the same reason
    /// as [`for_each_config_source`](Self::for_each_config_source):
    /// `__()` return types are resolved through the shared loaders, which
    /// run inside the workspace index, so ensuring the index here would
    /// re-enter its lock. Files open in the editor but not yet on disk are
    /// taken from the already-parsed snapshot, without blocking.
    fn enumerate_all_trans_key_shapes(&self) -> HashMap<String, bool> {
        let mut lang_uris: Vec<String> = Vec::new();
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            let vendor_dir_paths = self.workspace.vendor_dir_paths.lock().clone();
            let filters = self.index_filters();
            for path in crate::classmap_scanner::collect_php_files_gitignore(
                &root,
                &vendor_dir_paths,
                &filters,
                Some(self.followed_links()),
            ) {
                let uri = crate::util::path_to_uri(&path);
                if is_lang_php_uri(&uri) {
                    lang_uris.push(uri);
                }
            }
        }
        for (uri, _) in self.user_file_symbol_maps_nonblocking() {
            if is_lang_php_uri(&uri) && !lang_uris.contains(&uri) {
                lang_uris.push(uri);
            }
        }

        let mut shapes = HashMap::new();
        for file_uri in &lang_uris {
            let Some(stem) = extract_lang_file_stem(file_uri) else {
                continue;
            };
            let Some(content) = self.get_file_content_arc(file_uri) else {
                continue;
            };
            let decls =
                crate::virtual_members::laravel::collect_trans_declarations(&content, &stem);
            for d in decls {
                mark_trans_shape(&mut shapes, d.key, d.is_group);
            }
        }

        collect_json_trans_key_shapes(self, &mut shapes);

        for res in &self.laravel_provider_resources.read().trans_dirs {
            collect_namespaced_trans_key_shapes(&res.path, &res.namespace, &mut shapes);
        }

        shapes
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

    /// Every translation key, sorted.
    pub(crate) fn cached_trans_keys(&self) -> std::sync::Arc<[String]> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.trans_keys,
            |cache| cache.trans_keys.clone(),
            |cache, keys| cache.trans_keys = Some(keys),
            || {
                let mut keys: Vec<String> =
                    self.cached_trans_key_shapes().keys().cloned().collect();
                keys.sort();
                keys.into()
            },
        )
    }

    /// Every translation key mapped to whether it names a group (nested
    /// array) rather than a scalar entry.  Used to narrow the return type
    /// of `__()`/`trans()`/`Lang::get()` at call sites whose key argument
    /// is a literal.
    pub(crate) fn cached_trans_key_shapes(&self) -> std::sync::Arc<HashMap<String, bool>> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.trans_key_shapes,
            |cache| cache.trans_key_shapes.clone(),
            |cache, shapes| cache.trans_key_shapes = Some(shapes),
            || std::sync::Arc::new(self.enumerate_all_trans_key_shapes()),
        )
    }
}

/// Extract the file stem from a lang file URI for use as the translation
/// key prefix.
///
/// `file:///path/lang/en/messages.php` → `"messages"`
pub(super) fn extract_lang_file_stem(uri: &str) -> Option<String> {
    let file = uri.rsplit('/').next()?;
    let stem = file.strip_suffix(".php")?;
    if stem.is_empty() {
        return None;
    }
    Some(stem.to_string())
}

/// Whether a user-file URI is a PHP translation file.
fn is_lang_php_uri(uri: &str) -> bool {
    (uri.contains("/lang/") || uri.contains("/resources/lang/")) && uri.ends_with(".php")
}

/// Record a key's group/scalar shape, OR-ing into any flag already
/// recorded for the same key from another locale or file.
fn mark_trans_shape(shapes: &mut HashMap<String, bool>, key: String, is_group: bool) {
    let existing = shapes.entry(key).or_insert(false);
    *existing = *existing || is_group;
}

/// Record the top-level keys of the workspace's `lang/*.json` files into
/// `out`.  A JSON translation is a flat phrase-to-line map, so every key
/// is a scalar, never a group.
fn collect_json_trans_key_shapes(backend: &crate::Backend, out: &mut HashMap<String, bool>) {
    let Some(root) = backend.workspace.workspace_root.read().clone() else {
        return;
    };
    crate::virtual_members::laravel::for_each_json_lang_file(&root, |_, map| {
        for k in map.keys() {
            mark_trans_shape(out, k.clone(), false);
        }
    });
}

/// Scan a package translation directory and record keys in
/// `namespace::file.key` format (PHP files) or `namespace::raw_key`
/// (JSON files with empty namespace).
fn collect_namespaced_trans_key_shapes(
    dir: &std::path::Path,
    namespace: &str,
    out: &mut HashMap<String, bool>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_namespaced_trans_shapes_from_locale_dir(&path, namespace, out);
        } else if path.extension().is_some_and(|e| e == "json")
            && namespace.is_empty()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(map) =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content)
        {
            for k in map.keys() {
                mark_trans_shape(out, k.clone(), false);
            }
        }
    }
}

fn collect_namespaced_trans_shapes_from_locale_dir(
    dir: &std::path::Path,
    namespace: &str,
    out: &mut HashMap<String, bool>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e == "php") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let prefix = if namespace.is_empty() {
            stem.to_string()
        } else {
            format!("{namespace}::{stem}")
        };
        let decls = crate::virtual_members::laravel::collect_trans_declarations(&content, &prefix);
        for d in decls {
            mark_trans_shape(out, d.key, d.is_group);
        }
    }
}
