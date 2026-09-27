use std::borrow::Cow;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::Backend;
use crate::references::push_location;
use crate::symbol_map::{LaravelStringKind, SymbolKind, SymbolMap, SymbolSpan};
use crate::text_position::{LineIndex, offset_to_position};

#[derive(Debug)]
pub(crate) struct ConfigKeyMatch {
    pub key: String,
    pub start: usize,
    pub end: usize,
}

/// Try to determine the dot-notated configuration prefix for a given file URI.
///
/// For example, `file:///path/to/project/config/app.php` returns `Some("app")`.
/// Supports nested directories: `config/api/keys.php` returns `Some("api.keys")`.
pub(crate) fn laravel_config_prefix_from_uri(uri: &str) -> Option<String> {
    let parsed = Url::parse(uri).ok()?;
    // Match the nearest `config` directory to the file path. This avoids
    // false negatives when an ancestor directory is also named `config`.
    let relative = parsed.path().rsplit_once("/config/")?.1;
    let stem = relative.strip_suffix(".php")?;
    if stem.is_empty() {
        return None;
    }
    Some(stem.replace('/', "."))
}

/// Collect Laravel config declaration keys from a `config/*.php` file.
///
/// Produces keys in dot notation (`app.mail.from.address`) and records
/// source spans for the key literal content (inside quotes).
pub(crate) fn collect_laravel_config_declarations(
    content: &str,
    prefix: &str,
) -> Vec<ConfigKeyMatch> {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let mut out = Vec::new();
    for expr in super::array_file::returned_exprs(program) {
        super::array_file::for_each_entry(expr, content, &mut |path, start, end, _value| {
            out.push(ConfigKeyMatch {
                key: super::array_file::dotted_key(prefix, path),
                start,
                end,
            });
        });
    }
    out
}

/// Where a config file comes from, which decides how Laravel merges it
/// beneath the files that take precedence over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigSourceKind {
    /// A file in the application's own `config/` directory.
    Project,
    /// A package file a service provider registers with `mergeConfigFrom()`.
    Package,
    /// One of the framework's own defaults, which `LoadConfiguration`
    /// merges beneath the application's file of the same name.
    Framework,
}

/// The directory holding the framework's default config files.
fn framework_config_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join("vendor/laravel/framework/config")
}

// ─── Keys declared at runtime ─────────────────────────────────────────────────

/// The config keys a single file declares at runtime, read from the
/// [`SymbolKind::LaravelStringKey`] spans the extractor already marked as
/// writes.
fn config_write_keys(symbol_map: &SymbolMap) -> Vec<String> {
    let mut keys: Vec<String> = symbol_map
        .spans
        .iter()
        .filter_map(|span| match &span.kind {
            SymbolKind::LaravelStringKey {
                kind,
                key,
                is_write: true,
                ..
            } if kind.is_config_backed() => Some(canonical_config_key(kind, key).into_owned()),
            _ => None,
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

impl Backend {
    /// Record the runtime config writes from each map being published, so a
    /// later read is judged against the same active spans as navigation.
    ///
    /// This includes batch indexing and maps whose facade alias was shadowed
    /// or restored by an edit elsewhere. Each file's whole set is replaced:
    /// removing a write must also withdraw the key it declared. Vendor maps
    /// are excluded because their on-demand loading would otherwise make
    /// diagnostics depend on which classes happened to be loaded.
    pub(crate) fn refresh_laravel_config_writes<'a>(
        &self,
        maps: impl IntoIterator<Item = (&'a str, &'a SymbolMap)>,
    ) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        for (uri, map) in maps {
            let keys = config_write_keys(map);
            if keys.is_empty() {
                // Only take the write lock when there is something to forget.
                if self.laravel_runtime_config_keys.read().contains_key(uri) {
                    self.laravel_runtime_config_keys.write().remove(uri);
                }
                continue;
            }
            if self
                .workspace
                .vendor_uri_prefixes
                .lock()
                .iter()
                .any(|prefix| uri.starts_with(prefix.as_str()))
            {
                continue;
            }
            let mut index = self.laravel_runtime_config_keys.write();
            if index.get(uri) != Some(&keys) {
                index.insert(uri.to_string(), keys);
            }
        }
    }

    /// Whether a config key the project declares at runtime covers `key`.
    ///
    /// `Config::set('filesystems.disks.ondemand', […])` in a test's `setUp()`
    /// establishes a key no `config/` file declares, and a read of it
    /// afterwards is as valid as a read of one that ships on disk.  The
    /// value a write stored is opaque, so every path under a written key
    /// is beyond judging as well, and a read of a group above one is as
    /// real as the write.
    pub(crate) fn runtime_config_key_covers(&self, key: &str) -> bool {
        self.laravel_runtime_config_keys
            .read()
            .values()
            .flatten()
            .any(|written| {
                written == key
                    || written
                        .strip_prefix(key)
                        .is_some_and(|rest| rest.starts_with('.'))
                    || key
                        .strip_prefix(written.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            })
    }

    /// Visit every config file a Laravel project reads, highest precedence
    /// first, with the key prefix its entries live under: the project's
    /// own `config/` files, then the files service providers register, then
    /// the framework's defaults.
    ///
    /// Config-key completion and config-type resolution both read the
    /// config through here, so they cannot see different files.
    ///
    /// The project's files are discovered with a direct disk walk rather
    /// than through `user_file_symbol_maps`, which forces the workspace
    /// index. Config-type resolution runs *inside* class loading
    /// (`patch_storage_disk_type`) and inside the blade injected-vars
    /// refresh the index itself performs, so ensuring the index there
    /// re-enters the index lock and the enumeration cache's own build lock
    /// and deadlocks. Only the files' contents are needed, not their symbol
    /// maps. Files that are open in the editor but not yet on disk are taken
    /// from the already-parsed snapshot, without blocking on the index.
    pub(crate) fn for_each_config_source(
        &self,
        mut visit: impl FnMut(&str, ConfigSourceKind, &str),
    ) {
        let workspace_root = self.workspace.workspace_root.read().clone();
        let mut config_uris: Vec<String> = Vec::new();
        if let Some(root) = &workspace_root {
            let vendor_dir_paths = self.workspace.vendor_dir_paths.lock().clone();
            let filters = self.index_filters();
            for path in crate::classmap_scanner::collect_php_files_gitignore(
                root,
                &vendor_dir_paths,
                &filters,
                Some(self.followed_links()),
            ) {
                let uri = crate::util::path_to_uri(&path);
                if laravel_config_prefix_from_uri(&uri).is_some() {
                    config_uris.push(uri);
                }
            }
        }
        for (uri, _) in self.user_file_symbol_maps_nonblocking() {
            if laravel_config_prefix_from_uri(&uri).is_some() && !config_uris.contains(&uri) {
                config_uris.push(uri);
            }
        }
        // Deterministic order regardless of walk or map order.
        config_uris.sort();
        for file_uri in &config_uris {
            let Some(prefix) = laravel_config_prefix_from_uri(file_uri) else {
                continue;
            };
            if let Some(content) = self.get_file_content_arc(file_uri) {
                visit(&prefix, ConfigSourceKind::Project, &content);
            }
        }

        for res in &self.laravel_provider_resources.read().config_files {
            if let Ok(content) = std::fs::read_to_string(&res.path) {
                visit(&res.namespace, ConfigSourceKind::Package, &content);
            }
        }

        let Some(root) = workspace_root else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(framework_config_dir(&root)) else {
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
            if let Ok(content) = std::fs::read_to_string(&path) {
                visit(stem, ConfigSourceKind::Framework, &content);
            }
        }
    }

    /// Whether the workspace is an application rather than a library; see
    /// [`Backend::is_application`](crate::Backend::is_application).
    pub(crate) fn is_application_project(&self) -> bool {
        self.is_application
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Record the application/library classification, once the workspace's
    /// `composer.json` files have been read.
    pub(crate) fn set_is_application(&self, is_application: bool) {
        self.is_application
            .store(is_application, std::sync::atomic::Ordering::Relaxed);
    }
}

// ─── Public cross-file query API ──────────────────────────────────────────────

/// Find all references for a Laravel config key across the project.
///
/// Uses pre-built [`SymbolKind::LaravelStringKey`] spans to avoid re-parsing
/// every file at request time (same pattern as `find_member_references`).
pub(crate) fn find_config_references(
    backend: &Backend,
    uri: &str,
    content: &str,
    position: Position,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    // Fast path: cursor is on a usage site — symbol map already has the key.
    let (target_kind, target_key) = if let Some(sym) =
        backend.lookup_symbol_at_position(uri, content, position)
    {
        match sym.kind {
            SymbolKind::LaravelStringKey { kind, key, .. } if kind.is_config_backed() => {
                (kind, key)
            }
            _ => return None,
        }
    } else {
        // Fallback: cursor is on a declaration key inside config/*.php.
        // This re-parses the current (single) config file — acceptable.
        let prefix = laravel_config_prefix_from_uri(uri)?;
        let cursor_offset = crate::text_position::position_to_offset(content, position) as usize;
        let key = collect_laravel_config_declarations(content, &prefix)
            .into_iter()
            .find(|d| cursor_offset >= d.start && cursor_offset <= d.end)
            .map(|d| d.key)?;
        (LaravelStringKind::Config, key)
    };

    let reference_key =
        crate::reference_index::laravel_string_reference_key(target_kind, &target_key);
    let snapshot = backend.user_file_symbol_maps_for_reference_keys(&[reference_key]);
    let locations = find_all_config_references(
        backend,
        &target_kind,
        &target_key,
        &snapshot,
        include_declaration,
    );

    if locations.is_empty() {
        return None;
    }

    Some(locations)
}

/// Called from `resolve_from_symbol` when the symbol map contains a
/// [`SymbolKind::LaravelStringKey`] span with `kind == Config` at the cursor —
/// no file re-parse is needed for the usage side.
///
/// The files that can declare the key are tried in the order Laravel lets
/// them win: the application's `config/` files, then the package files
/// service providers merge beneath them, then the framework's defaults.
/// The first that declares the key is the one whose value survives the
/// merge.  When none does, the key's own file is still the best place to
/// land.
pub(crate) fn resolve_config_key_declaration(backend: &Backend, key: &str) -> Option<Location> {
    resolve_config_key_declaration_inner(backend, key, true)
}

/// Resolve an exact config entry without falling back to the owning file.
///
/// Named resources use this path so a misspelled resource never jumps to
/// line zero of an otherwise-valid config file.
pub(crate) fn resolve_config_key_declaration_exact(
    backend: &Backend,
    key: &str,
) -> Option<Location> {
    resolve_config_key_declaration_inner(backend, key, false)
}

fn resolve_config_key_declaration_inner(
    backend: &Backend,
    key: &str,
    allow_file_fallback: bool,
) -> Option<Location> {
    let parts: Vec<&str> = key.split('.').collect();
    let root = backend.workspace.workspace_root.read().clone()?;

    let mut candidates: Vec<(String, std::path::PathBuf)> = Vec::new();
    let config_dir = root.join("config");
    for i in 1..=parts.len() {
        let file_parts = &parts[..i];
        let path = config_dir.join(format!("{}.php", file_parts.join("/")));
        if path.is_file() {
            candidates.push((file_parts.join("."), path));
        }
    }
    for res in &backend.laravel_provider_resources.read().config_files {
        let covers = key
            .strip_prefix(res.namespace.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
        if covers && res.path.is_file() {
            candidates.push((res.namespace.clone(), res.path.clone()));
        }
    }
    if let Some(first) = parts.first() {
        let path = framework_config_dir(&root).join(format!("{first}.php"));
        if path.is_file() {
            candidates.push((first.to_string(), path));
        }
    }

    let mut fallback = None;
    for (prefix, path) in candidates {
        let Ok(target_uri) = Url::from_file_path(&path) else {
            continue;
        };
        let Some(target_content) = backend.get_file_content_arc(target_uri.as_str()) else {
            continue;
        };
        let declarations = collect_laravel_config_declarations(&target_content, &prefix);
        if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
            return Some(config_declaration_location(
                target_uri,
                &target_content,
                &decl,
            ));
        }
        if allow_file_fallback {
            fallback.get_or_insert(target_uri);
        }
    }

    fallback.map(|uri| crate::definition::point_location(uri, Position::new(0, 0)))
}

fn config_declaration_location(uri: Url, content: &str, declaration: &ConfigKeyMatch) -> Location {
    Location {
        uri,
        range: Range::new(
            offset_to_position(content, declaration.start),
            offset_to_position(content, declaration.end),
        ),
    }
}

/// Find all references for a Laravel config key across the project.
///
/// Iterates pre-built [`SymbolKind::LaravelStringKey`] spans for usages
/// (zero re-parses per file, same pattern as `find_member_references`).
/// Declaration lookup parses only the config file that can own the canonical
/// key, independently of the usage-candidate snapshot.
pub(crate) fn find_all_config_references(
    backend: &Backend,
    target_kind: &LaravelStringKind,
    target_key: &str,
    snapshot: &[(String, Arc<SymbolMap>)],
    include_declaration: bool,
) -> Vec<Location> {
    if !target_kind.is_config_backed() {
        return Vec::new();
    }
    let mut locations = Vec::new();

    // Usages: walk pre-built symbol spans — no file re-parse needed.
    for (file_uri, symbol_map) in snapshot {
        let parsed_uri = match Url::parse(file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };
        // Read only once a span matches, and convert every match through
        // one line table rather than rescanning the file per hit.
        let file_content = std::cell::OnceCell::new();
        let lines = std::cell::OnceCell::new();
        for span in &symbol_map.spans {
            if config_span_matches(span, target_kind, target_key) {
                let Some(content) = file_content
                    .get_or_init(|| backend.get_file_content_arc(file_uri))
                    .as_ref()
                else {
                    break;
                };
                let lines = lines.get_or_init(|| LineIndex::new(content));
                let start = lines.position(span.start as usize);
                let end = lines.position(span.end as usize);
                push_location(&mut locations, &parsed_uri, start, end);
            }
        }
    }

    // Declarations: keys in config/*.php (small set, AST walk acceptable).
    if include_declaration {
        let canonical_key = canonical_config_key(target_kind, target_key);
        if let Some(declaration) =
            resolve_config_key_declaration_exact(backend, canonical_key.as_ref())
        {
            push_location(
                &mut locations,
                &declaration.uri,
                declaration.range.start,
                declaration.range.end,
            );
        }
    }

    locations
}

fn config_span_matches(
    span: &SymbolSpan,
    target_kind: &LaravelStringKind,
    target_key: &str,
) -> bool {
    matches!(
        &span.kind,
        SymbolKind::LaravelStringKey { kind, key, .. }
            if config_keys_match(target_kind, target_key, kind, key)
    )
}

fn config_keys_match(
    left_kind: &LaravelStringKind,
    left_key: &str,
    right_kind: &LaravelStringKind,
    right_key: &str,
) -> bool {
    match (left_kind, right_kind) {
        (LaravelStringKind::Config, LaravelStringKind::Config) => left_key == right_key,
        (LaravelStringKind::ConfigResource(left), LaravelStringKind::ConfigResource(right)) => {
            left == right
                && crate::symbol_map::laravel_resources::same_resource_name(
                    *left, left_key, right_key,
                )
        }
        (LaravelStringKind::Config, LaravelStringKind::ConfigResource(resource)) => {
            crate::symbol_map::laravel_resources::matches_config_key(*resource, right_key, left_key)
        }
        (LaravelStringKind::ConfigResource(resource), LaravelStringKind::Config) => {
            crate::symbol_map::laravel_resources::matches_config_key(*resource, left_key, right_key)
        }
        _ => false,
    }
}

fn canonical_config_key<'a>(kind: &LaravelStringKind, key: &'a str) -> Cow<'a, str> {
    match kind {
        LaravelStringKind::ConfigResource(resource) => Cow::Owned(
            crate::symbol_map::laravel_resources::config_key(*resource, key),
        ),
        _ => Cow::Borrowed(key),
    }
}

/// Fallback for "go to definition" on a key inside config/*.php.
///
/// Since array keys are not indexed in the symbol map, the generic
/// resolution returns None.  This re-parses the current file to see
/// if the cursor is on a known config key, and if so, returns a Location
/// pointing to the same file (enabling Find All References for that key).
pub(crate) fn resolve_config_key_definition_fallback(
    _backend: &Backend,
    uri: &str,
    content: &str,
    position: Position,
) -> Option<Location> {
    let prefix = laravel_config_prefix_from_uri(uri)?;
    let cursor_offset = crate::text_position::position_to_offset(content, position) as usize;
    let decls = collect_laravel_config_declarations(content, &prefix);
    let match_ = decls
        .into_iter()
        .find(|d| cursor_offset >= d.start && cursor_offset <= d.end)?;

    let target_uri = Url::parse(uri).ok()?;
    let pos = crate::text_position::offset_to_position(content, match_.start);
    Some(crate::definition::point_location(target_uri, pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The index tracks the file, not the key: an edit that takes the write
    /// away has to take what it declared with it, or the key outlives the
    /// call that made it.
    #[test]
    fn a_runtime_write_lasts_exactly_as_long_as_the_call_that_makes_it() {
        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(true);
        let uri = "file:///project/tests/FixtureTest.php";

        backend.update_ast(
            uri,
            &Arc::new(
                "<?php\nConfig::set('filesystems.disks.ondemand', []);\nStorage::fake('scratch');\nStorage::persistentFake('persistent');\n"
                    .to_string(),
            ),
        );
        assert!(
            backend.runtime_config_key_covers("filesystems.disks.ondemand"),
            "the write should declare the disk it configures"
        );
        for key in ["filesystems.disks.scratch", "filesystems.disks.persistent"] {
            assert!(backend.runtime_config_key_covers(key));
        }

        backend.update_ast(uri, &Arc::new("<?php\nclass FixtureTest {}\n".to_string()));
        assert!(
            !backend.runtime_config_key_covers("filesystems.disks.ondemand"),
            "removing the write should remove the key it declared"
        );
    }

    /// A deleted file is never re-parsed, so the per-file eviction has to
    /// forget its keys rather than leave them to the next refresh.
    #[test]
    fn a_deleted_file_takes_its_runtime_writes_with_it() {
        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(true);
        let uri = "file:///project/tests/FixtureTest.php";

        backend.update_ast(
            uri,
            &Arc::new("<?php\nConfig::set('filesystems.disks.ondemand', []);\n".to_string()),
        );
        assert!(backend.runtime_config_key_covers("filesystems.disks.ondemand"));

        backend.clear_file_maps(uri);
        assert!(
            !backend.runtime_config_key_covers("filesystems.disks.ondemand"),
            "clearing the file's maps should drop the keys it declared"
        );
    }

    #[test]
    fn vendor_runtime_writes_do_not_hide_application_writes_in_the_same_batch() {
        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(true);
        backend
            .workspace
            .vendor_uri_prefixes
            .lock()
            .push("file:///project/vendor/".to_string());
        let vendor_uri = "file:///project/vendor/package/Fixture.php";
        let app_uri = "file:///project/app/Fixture.php";
        let vendor = backend.parse_ast_index_update_for_index(
            vendor_uri,
            "<?php Config::set('filesystems.disks.vendor-only', []);",
        );
        let application = backend
            .parse_ast_index_update_for_index(app_uri, "<?php Storage::fake('application-only');");

        backend.apply_ast_index_parse_results_batch(vec![vendor, application]);

        let maps = backend.symbol_maps.read();
        assert_eq!(
            config_write_keys(&maps[vendor_uri]),
            ["filesystems.disks.vendor-only"],
            "the vendor write was parsed, but must not declare application configuration"
        );
        assert_eq!(
            *backend.laravel_runtime_config_keys.read(),
            std::collections::HashMap::from([(
                app_uri.to_string(),
                vec!["filesystems.disks.application-only".to_string()],
            )])
        );
        assert!(
            !backend
                .laravel_runtime_config_keys
                .read()
                .contains_key(vendor_uri)
        );
    }

    #[test]
    fn config_prefix_from_uri_normal() {
        assert_eq!(
            laravel_config_prefix_from_uri("file:///project/config/app.php"),
            Some("app".to_string())
        );
    }

    #[test]
    fn config_prefix_from_uri_root_level() {
        assert_eq!(
            laravel_config_prefix_from_uri("file:///config/app.php"),
            Some("app".to_string())
        );
    }

    #[test]
    fn config_prefix_from_uri_not_in_config_dir() {
        assert_eq!(
            laravel_config_prefix_from_uri("file:///project/src/Service.php"),
            None
        );
    }

    #[test]
    fn config_prefix_from_uri_file_named_config() {
        assert_eq!(
            laravel_config_prefix_from_uri("file:///project/config.php"),
            None
        );
    }

    #[test]
    fn config_prefix_from_uri_supports_subdirectory() {
        assert_eq!(
            laravel_config_prefix_from_uri("file:///project/config/mail/transport.php"),
            Some("mail.transport".to_string())
        );
    }

    #[test]
    fn config_prefix_from_uri_uses_nearest_config_segment() {
        assert_eq!(
            laravel_config_prefix_from_uri(
                "file:///workspace/config/vendor/project/config/app.php"
            ),
            Some("app".to_string())
        );
    }

    #[test]
    fn test_collect_declarations_variable_return() {
        let content = "<?php
$config = [
    'name' => 'Laravel',
];
return $config;";
        let prefix = "app";
        let decls = collect_laravel_config_declarations(content, prefix);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].key, "app.name");
    }

    #[test]
    fn test_collect_declarations_array_merge() {
        let content = "<?php
return array_merge([
    'name' => 'Laravel',
], [
    'env' => 'production',
]);";
        let prefix = "app";
        let decls = collect_laravel_config_declarations(content, prefix);
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].key, "app.name");
        assert_eq!(decls[1].key, "app.env");
    }

    #[test]
    fn config_resource_keys_match_generic_config_keys_symmetrically() {
        use crate::symbol_map::LaravelConfigResource::{CacheStore, StorageDisk};

        let resource = LaravelStringKind::ConfigResource(CacheStore);
        assert!(config_keys_match(
            &resource,
            "redis",
            &LaravelStringKind::Config,
            "cache.stores.redis",
        ));
        assert!(config_keys_match(
            &LaravelStringKind::Config,
            "cache.stores.redis",
            &resource,
            "redis",
        ));
        assert!(!config_keys_match(
            &LaravelStringKind::ConfigResource(StorageDisk),
            "redis",
            &resource,
            "redis",
        ));
        assert!(config_keys_match(&resource, "redis", &resource, "redis"));
        assert!(!config_keys_match(
            &resource,
            "redis",
            &LaravelStringKind::Config,
            "cache.stores.redis.options",
        ));
        assert!(!config_keys_match(
            &LaravelStringKind::View,
            "redis",
            &LaravelStringKind::Config,
            "cache.stores.redis",
        ));
        let database = LaravelStringKind::ConfigResource(
            crate::symbol_map::LaravelConfigResource::DatabaseConnection,
        );
        assert!(config_keys_match(
            &database,
            "mysql::read",
            &database,
            "mysql::write",
        ));
        assert!(config_keys_match(
            &database,
            "mysql::direct",
            &LaravelStringKind::Config,
            "database.connections.mysql",
        ));
        assert!(!config_keys_match(
            &LaravelStringKind::ConfigResource(CacheStore),
            "null",
            &LaravelStringKind::Config,
            "cache.stores.null",
        ));
    }

    #[test]
    fn config_reference_scan_links_short_and_canonical_spans() {
        use crate::symbol_map::LaravelConfigResource::CacheStore;

        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("usage.php");
        let source = "redis cache.stores.redis";
        std::fs::write(&source_path, source).unwrap();
        let uri = crate::util::path_to_uri(&source_path);
        let backend = Backend::new_test();
        let map = Arc::new(SymbolMap {
            spans: vec![
                SymbolSpan {
                    start: 0,
                    end: 5,
                    kind: SymbolKind::LaravelStringKey {
                        kind: LaravelStringKind::ConfigResource(CacheStore),
                        key: "redis".to_string(),
                        is_write: false,
                        is_optional: false,
                    },
                },
                SymbolSpan {
                    start: 6,
                    end: source.len() as u32,
                    kind: SymbolKind::LaravelStringKey {
                        kind: LaravelStringKind::Config,
                        key: "cache.stores.redis".to_string(),
                        is_write: false,
                        is_optional: false,
                    },
                },
            ],
            ..SymbolMap::default()
        });
        let snapshot = [(uri.to_string(), map)];

        let resource = find_all_config_references(
            &backend,
            &LaravelStringKind::ConfigResource(CacheStore),
            "redis",
            &snapshot,
            false,
        );
        let generic = find_all_config_references(
            &backend,
            &LaravelStringKind::Config,
            "cache.stores.redis",
            &snapshot,
            false,
        );
        assert_eq!(resource, generic);
        assert_eq!(resource.len(), 2);
        assert_eq!(
            resource[0].range,
            Range::new(Position::new(0, 0), Position::new(0, 5))
        );
        assert_eq!(
            resource[1].range,
            Range::new(Position::new(0, 6), Position::new(0, source.len() as u32),)
        );
        assert!(
            find_all_config_references(
                &backend,
                &LaravelStringKind::View,
                "redis",
                &snapshot,
                false,
            )
            .is_empty()
        );
    }

    #[test]
    fn config_reference_entrypoint_accepts_a_resource_usage_span() {
        use crate::symbol_map::LaravelConfigResource::CacheStore;

        let backend = Backend::new_test();
        backend
            .workspace_indexed
            .store(true, std::sync::atomic::Ordering::Release);
        let uri = "file:///project/src/Consumer.php";
        let content = "redis";
        let map = Arc::new(SymbolMap {
            spans: vec![SymbolSpan {
                start: 0,
                end: content.len() as u32,
                kind: SymbolKind::LaravelStringKey {
                    kind: LaravelStringKind::ConfigResource(CacheStore),
                    key: content.to_string(),
                    is_write: false,
                    is_optional: false,
                },
            }],
            source_len: content.len() as u32,
            ..SymbolMap::default()
        });
        backend
            .open_files
            .write()
            .insert(uri.to_string(), Arc::new(content.to_string()));
        backend
            .symbol_maps
            .write()
            .insert(uri.to_string(), Arc::clone(&map));
        backend.reindex_references_for_symbol_maps_batch(vec![(uri.to_string(), map)]);

        let locations = find_config_references(&backend, uri, content, Position::new(0, 1), false)
            .expect("resource usage should resolve its references");
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].uri.as_str(), uri);
    }

    #[test]
    fn exact_config_lookup_uses_app_then_framework_and_never_file_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let app_config = dir.path().join("config/cache.php");
        let stale_provider_config = dir.path().join("vendor/stale/config/cache.php");
        let provider_config = dir.path().join("vendor/package/config/cache.php");
        let second_provider_config = dir.path().join("vendor/other/config/cache.php");
        let framework_config = dir.path().join("vendor/laravel/framework/config/cache.php");
        std::fs::create_dir_all(app_config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(provider_config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(second_provider_config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(framework_config.parent().unwrap()).unwrap();
        std::fs::write(
            &app_config,
            "<?php return ['stores' => ['tenant' => ['driver' => 'array']]];\n",
        )
        .unwrap();
        std::fs::write(
            &provider_config,
            "<?php return ['stores' => ['package' => ['driver' => 'array']]];\n",
        )
        .unwrap();
        std::fs::write(
            &second_provider_config,
            "<?php return ['stores' => ['second' => ['driver' => 'array']]];\n",
        )
        .unwrap();
        std::fs::write(
            &framework_config,
            "<?php return ['stores' => ['redis' => ['driver' => 'redis']]];\n",
        )
        .unwrap();

        let backend = Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
        backend
            .laravel_provider_resources
            .write()
            .config_files
            .extend([
                crate::virtual_members::laravel::ProviderResource {
                    path: stale_provider_config,
                    namespace: "cache".to_string(),
                },
                crate::virtual_members::laravel::ProviderResource {
                    path: provider_config.clone(),
                    namespace: "cache".to_string(),
                },
                crate::virtual_members::laravel::ProviderResource {
                    path: second_provider_config.clone(),
                    namespace: "cache".to_string(),
                },
            ]);

        let app = resolve_config_key_declaration_exact(&backend, "cache.stores.tenant").unwrap();
        assert_eq!(app.uri, Url::from_file_path(app_config).unwrap());
        assert_ne!(app.range.start, app.range.end);

        let provider =
            resolve_config_key_declaration_exact(&backend, "cache.stores.package").unwrap();
        assert_eq!(provider.uri, Url::from_file_path(provider_config).unwrap());

        let second_provider =
            resolve_config_key_declaration_exact(&backend, "cache.stores.second").unwrap();
        assert_eq!(
            second_provider.uri,
            Url::from_file_path(second_provider_config).unwrap()
        );

        let framework =
            resolve_config_key_declaration_exact(&backend, "cache.stores.redis").unwrap();
        assert_eq!(
            framework.uri,
            Url::from_file_path(framework_config).unwrap()
        );

        assert!(resolve_config_key_declaration_exact(&backend, "cache.stores.missing").is_none());
    }

    #[test]
    fn generic_config_lookup_can_fall_back_to_a_provider_file() {
        let dir = tempfile::tempdir().unwrap();
        let provider_config = dir.path().join("vendor/package/resources/cache.php");
        let framework_config = dir.path().join("vendor/laravel/framework/config/cache.php");
        std::fs::create_dir_all(provider_config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(framework_config.parent().unwrap()).unwrap();
        std::fs::write(
            &provider_config,
            "<?php return ['stores' => ['shared' => ['driver' => 'array']]];\n",
        )
        .unwrap();
        std::fs::write(
            &framework_config,
            "<?php return ['stores' => ['shared' => ['driver' => 'redis']]];\n",
        )
        .unwrap();

        let backend = Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
        backend
            .laravel_provider_resources
            .write()
            .config_files
            .push(crate::virtual_members::laravel::ProviderResource {
                path: provider_config.clone(),
                namespace: "cache".to_string(),
            });

        let exact = resolve_config_key_declaration_exact(&backend, "cache.stores.shared").unwrap();
        assert_eq!(exact.uri, Url::from_file_path(&provider_config).unwrap());
        assert!(resolve_config_key_declaration_exact(&backend, "cache.stores.missing").is_none());

        let fallback = resolve_config_key_declaration(&backend, "cache.stores.missing").unwrap();
        assert_eq!(fallback.uri, Url::from_file_path(provider_config).unwrap());
        assert_eq!(
            fallback.range,
            Range::new(Position::new(0, 0), Position::new(0, 0))
        );
    }
}
