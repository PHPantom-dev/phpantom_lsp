use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;
use crate::references::push_location;
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::text_position::LineIndex;

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
    let path = parsed.path();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // Match the nearest `config` directory to the file path. This avoids
    // false negatives when an ancestor directory is also named `config`.
    let config_idx = segments.iter().rposition(|seg| *seg == "config")?;
    let file = segments.last()?;
    if !file.ends_with(".php") {
        return None;
    }

    let prefix_segments = &segments[config_idx + 1..];
    if prefix_segments.is_empty() {
        return None;
    }

    let mut stem_segments: Vec<String> = prefix_segments.iter().map(|s| s.to_string()).collect();
    let last = stem_segments.last_mut()?;
    *last = last.strip_suffix(".php")?.to_string();

    if last.is_empty() {
        return None;
    }

    Some(stem_segments.join("."))
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
                kind: crate::symbol_map::LaravelStringKind::Config,
                key,
                is_write: true,
                ..
            } => Some(key.clone()),
            _ => None,
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

impl Backend {
    /// Record which config keys `uri` declares at runtime, so a later read of
    /// one is judged against it.
    ///
    /// Called after every re-parse: a write the edit removed has to take its
    /// key with it, which is why the file's whole set is replaced rather than
    /// merged.  Vendor files are left out for the same reason the enumeration
    /// of `config/` files leaves them out: they are parsed on demand, so
    /// including them would make what the diagnostic knows depend on which
    /// classes happened to be loaded.
    pub(crate) fn refresh_laravel_config_writes(&self, uri: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let keys = self
            .symbol_maps
            .read()
            .get(uri)
            .map(|map| config_write_keys(map))
            .unwrap_or_default();

        if keys.is_empty() {
            // Only take the write lock when there is something to forget.
            if self.laravel_runtime_config_keys.read().contains_key(uri) {
                self.laravel_runtime_config_keys.write().remove(uri);
            }
            return;
        }
        if self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
        {
            return;
        }
        let mut index = self.laravel_runtime_config_keys.write();
        if index.get(uri) != Some(&keys) {
            index.insert(uri.to_string(), keys);
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
    let target_key = if let Some(sym) = backend.lookup_symbol_at_position(uri, content, position) {
        match sym.kind {
            SymbolKind::LaravelStringKey { key, .. } => key,
            _ => return None,
        }
    } else {
        // Fallback: cursor is on a declaration key inside config/*.php.
        // This re-parses the current (single) config file — acceptable.
        let prefix = laravel_config_prefix_from_uri(uri)?;
        let cursor_offset = crate::text_position::position_to_offset(content, position) as usize;
        collect_laravel_config_declarations(content, &prefix)
            .into_iter()
            .find(|d| cursor_offset >= d.start && cursor_offset <= d.end)
            .map(|d| d.key)?
    };

    let snapshot = backend.user_file_symbol_maps();
    let locations =
        find_all_config_references(backend, &target_key, &snapshot, include_declaration);

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
            let pos = crate::text_position::offset_to_position(&target_content, decl.start);
            return Some(crate::definition::point_location(target_uri, pos));
        }
        fallback.get_or_insert(target_uri);
    }

    fallback.map(|uri| crate::definition::point_location(uri, Position::new(0, 0)))
}

/// Find all references for a Laravel config key across the project.
///
/// Iterates pre-built [`SymbolKind::LaravelStringKey`] spans for usages
/// (zero re-parses per file, same pattern as `find_member_references`).
/// Declaration lookup in `config/*.php` still uses an AST walk, but that
/// set is small (typically < 20 files) and each parse is cheap.
pub(crate) fn find_all_config_references(
    backend: &Backend,
    target_key: &str,
    snapshot: &[(String, Arc<SymbolMap>)],
    include_declaration: bool,
) -> Vec<Location> {
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
            if let SymbolKind::LaravelStringKey {
                kind: crate::symbol_map::LaravelStringKind::Config,
                key,
                ..
            } = &span.kind
                && key == target_key
            {
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
        for (file_uri, _) in snapshot {
            let prefix = match laravel_config_prefix_from_uri(file_uri) {
                Some(p) => p,
                None => continue,
            };
            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };
            let file_content = match backend.get_file_content_arc(file_uri) {
                Some(c) => c,
                None => continue,
            };
            let lines = std::cell::OnceCell::new();
            for decl in collect_laravel_config_declarations(&file_content, &prefix) {
                if decl.key != target_key {
                    continue;
                }
                let lines = lines.get_or_init(|| LineIndex::new(&file_content));
                let start = lines.position(decl.start);
                let end = lines.position(decl.end);
                push_location(&mut locations, &parsed_uri, start, end);
            }
        }
    }

    locations
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
            &Arc::new("<?php\nConfig::set('filesystems.disks.ondemand', []);\n".to_string()),
        );
        assert!(
            backend.runtime_config_key_covers("filesystems.disks.ondemand"),
            "the write should declare the disk it configures"
        );

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
}
