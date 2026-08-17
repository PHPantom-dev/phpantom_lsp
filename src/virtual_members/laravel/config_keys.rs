use std::borrow::Cow;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::references::push_unique_location;
use crate::symbol_map::{LaravelStringKind, SymbolKind, SymbolMap, SymbolSpan};
use crate::text_position::offset_to_position;

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

    let mut returned_var_name: Option<&str> = None;
    let mut return_expr: Option<&Expression<'_>> = None;

    for stmt in program.statements.iter() {
        if let Statement::Return(ret) = stmt {
            if let Some(val) = ret.value {
                match val {
                    Expression::Variable(Variable::Direct(dv)) => {
                        returned_var_name = Some(bytes_to_str(dv.name));
                    }
                    _ => {
                        return_expr = Some(val);
                    }
                }
            }
            break;
        }
    }

    let mut path = Vec::new();
    if let Some(expr) = return_expr {
        collect_expr_declarations(expr, content, prefix, &mut path, &mut out);
    } else if let Some(var_name) = returned_var_name {
        for stmt in program.statements.iter() {
            if let Statement::Expression(expr_stmt) = stmt
                && let Expression::Assignment(assign) = expr_stmt.expression
                && let Expression::Variable(Variable::Direct(dv)) = assign.lhs
                && bytes_to_str(dv.name) == var_name
            {
                collect_expr_declarations(assign.rhs, content, prefix, &mut path, &mut out);
            }
        }
    }

    out
}

// ─── Declaration walker ───────────────────────────────────────────────────────

fn collect_expr_declarations<'content>(
    expr: &Expression<'_>,
    content: &'content str,
    prefix: &str,
    path: &mut Vec<&'content str>,
    out: &mut Vec<ConfigKeyMatch>,
) {
    match expr {
        Expression::Array(arr) => {
            collect_array_declarations(arr.elements.iter(), content, prefix, path, out);
        }
        Expression::LegacyArray(arr) => {
            collect_array_declarations(arr.elements.iter(), content, prefix, path, out);
        }
        Expression::Parenthesized(p) => {
            collect_expr_declarations(p.expression, content, prefix, path, out);
        }
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"array_merge")
            {
                for arg in fc.argument_list.arguments.iter() {
                    let arg_expr = match arg {
                        Argument::Positional(pos) => pos.value,
                        Argument::Named(named) => named.value,
                    };
                    collect_expr_declarations(arg_expr, content, prefix, path, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_array_declarations<'a, 'content>(
    elements: impl Iterator<Item = &'a ArrayElement<'a>>,
    content: &'content str,
    prefix: &str,
    path: &mut Vec<&'content str>,
    out: &mut Vec<ConfigKeyMatch>,
) {
    for element in elements {
        let ArrayElement::KeyValue(kv) = element else {
            continue;
        };
        let (key_text, key_start, key_end) =
            match super::helpers::extract_string_literal(kv.key, content) {
                Some(k) => k,
                None => continue,
            };

        let capacity = prefix.len()
            + path.iter().map(|segment| segment.len() + 1).sum::<usize>()
            + key_text.len()
            + 1;
        let mut dot_key = String::with_capacity(capacity);
        dot_key.push_str(prefix);
        for segment in path.iter().copied().chain(std::iter::once(key_text)) {
            dot_key.push('.');
            dot_key.push_str(segment);
        }
        out.push(ConfigKeyMatch {
            key: dot_key,
            start: key_start,
            end: key_end,
        });

        path.push(key_text);
        collect_expr_declarations(kv.value, content, prefix, path, out);
        path.pop();
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
    let config_dir = root.join("config");

    for i in 1..=parts.len() {
        let (file_parts, _) = parts.split_at(i);
        let rel_path = file_parts.join("/");
        let config_path = config_dir.join(format!("{}.php", rel_path));

        if config_path.is_file() {
            let target_uri = Url::from_file_path(&config_path).ok()?;
            let target_uri_string = target_uri.to_string();
            let target_content = backend
                .get_file_content(&target_uri_string)
                .or_else(|| std::fs::read_to_string(&config_path).ok())?;

            let stem = file_parts.join(".");
            let declarations = collect_laravel_config_declarations(&target_content, &stem);
            if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
                return Some(config_declaration_location(
                    target_uri,
                    &target_content,
                    &decl,
                ));
            }

            if allow_file_fallback {
                return Some(crate::definition::point_location(
                    target_uri,
                    Position::new(0, 0),
                ));
            }
        }
    }

    let first_part = parts.first()?;
    let provider_configs: Vec<_> = backend
        .laravel_provider_resources
        .read()
        .config_files
        .iter()
        .filter(|resource| resource.namespace == *first_part)
        .cloned()
        .collect();
    for res in &provider_configs {
        if res.namespace == *first_part && res.path.is_file() {
            let target_uri = Url::from_file_path(&res.path).ok()?;
            let target_content = std::fs::read_to_string(&res.path).ok()?;
            let declarations = collect_laravel_config_declarations(&target_content, &res.namespace);
            if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
                return Some(config_declaration_location(
                    target_uri,
                    &target_content,
                    &decl,
                ));
            }
            if allow_file_fallback {
                return Some(crate::definition::point_location(
                    target_uri,
                    Position::new(0, 0),
                ));
            }
        }
    }

    // Laravel's unpublished defaults are completion candidates, so an exact
    // configured resource must navigate to the same framework declaration
    // when the application and its providers do not override it.
    let framework_path = root
        .join("vendor/laravel/framework/config")
        .join(format!("{first_part}.php"));
    if framework_path.is_file() {
        let target_uri = Url::from_file_path(&framework_path).ok()?;
        let target_content = std::fs::read_to_string(&framework_path).ok()?;
        let declarations = collect_laravel_config_declarations(&target_content, first_part);
        if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
            return Some(config_declaration_location(
                target_uri,
                &target_content,
                &decl,
            ));
        }
    }

    None
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
        let file_content = match backend.get_file_content_arc(file_uri) {
            Some(c) => c,
            None => continue,
        };
        for span in &symbol_map.spans {
            if config_span_matches(span, target_kind, target_key) {
                let start = offset_to_position(&file_content, span.start as usize);
                let end = offset_to_position(&file_content, span.end as usize);
                push_unique_location(&mut locations, &parsed_uri, start, end);
            }
        }
    }

    // Declarations: keys in config/*.php (small set, AST walk acceptable).
    if include_declaration {
        let canonical_key = canonical_config_key(target_kind, target_key);
        if let Some(declaration) =
            resolve_config_key_declaration_exact(backend, canonical_key.as_ref())
        {
            push_unique_location(
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
