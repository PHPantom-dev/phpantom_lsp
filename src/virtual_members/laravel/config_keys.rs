use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::util::{offset_to_position, push_unique_location};

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

    // Prefix is everything from config_idx + 1 to the end, joined by dots.
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

    let mut returned_var_name: Option<String> = None;
    let mut return_expr: Option<&Expression<'_>> = None;

    for stmt in program.statements.iter() {
        if let Statement::Return(ret) = stmt {
            if let Some(val) = ret.value {
                match val {
                    Expression::Variable(Variable::Direct(dv)) => {
                        returned_var_name = Some(bytes_to_str(dv.name).to_string());
                    }
                    _ => {
                        return_expr = Some(val);
                    }
                }
            }
            break;
        }
    }

    if let Some(expr) = return_expr {
        collect_expr_declarations(expr, content, prefix, &[], &mut out);
    } else if let Some(var_name) = returned_var_name {
        for stmt in program.statements.iter() {
            if let Statement::Expression(expr_stmt) = stmt
                && let Expression::Assignment(assign) = expr_stmt.expression
                && let Expression::Variable(Variable::Direct(dv)) = assign.lhs
                && dv.name == var_name.as_bytes()
            {
                collect_expr_declarations(assign.rhs, content, prefix, &[], &mut out);
            }
        }
    }

    out
}

// ─── Declaration walker ───────────────────────────────────────────────────────

fn collect_expr_declarations(
    expr: &Expression<'_>,
    content: &str,
    prefix: &str,
    path: &[String],
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

fn collect_array_declarations<'a>(
    elements: impl Iterator<Item = &'a ArrayElement<'a>>,
    content: &str,
    prefix: &str,
    path: &[String],
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

        let mut full_path = path.to_vec();
        full_path.push(key_text.to_string());
        let dot_key = format!("{prefix}.{}", full_path.join("."));
        out.push(ConfigKeyMatch {
            key: dot_key,
            start: key_start,
            end: key_end,
        });

        collect_expr_declarations(kv.value, content, prefix, &full_path, out);
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
        let cursor_offset = crate::util::position_to_offset(content, position) as usize;
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
pub(crate) fn resolve_config_key_declaration(backend: &Backend, key: &str) -> Option<Location> {
    let parts: Vec<&str> = key.split('.').collect();
    let root = backend.workspace_root.read().clone()?;
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
                let pos = crate::util::offset_to_position(&target_content, decl.start);
                return Some(crate::definition::point_location(target_uri, pos));
            }

            return Some(crate::definition::point_location(
                target_uri,
                Position::new(0, 0),
            ));
        }
    }

    let first_part = parts.first()?;
    for res in &backend.laravel_provider_resources.read().config_files {
        if res.namespace == *first_part && res.path.is_file() {
            let target_uri = Url::from_file_path(&res.path).ok()?;
            let target_content = std::fs::read_to_string(&res.path).ok()?;
            let declarations = collect_laravel_config_declarations(&target_content, &res.namespace);
            if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
                let pos = crate::util::offset_to_position(&target_content, decl.start);
                return Some(crate::definition::point_location(target_uri, pos));
            }
            return Some(crate::definition::point_location(
                target_uri,
                Position::new(0, 0),
            ));
        }
    }

    None
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
        let file_content = match backend.get_file_content_arc(file_uri) {
            Some(c) => c,
            None => continue,
        };
        for span in &symbol_map.spans {
            if let SymbolKind::LaravelStringKey {
                kind: crate::symbol_map::LaravelStringKind::Config,
                key,
            } = &span.kind
                && key == target_key
            {
                let start = offset_to_position(&file_content, span.start as usize);
                let end = offset_to_position(&file_content, span.end as usize);
                push_unique_location(&mut locations, &parsed_uri, start, end);
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
            for decl in collect_laravel_config_declarations(&file_content, &prefix) {
                if decl.key != target_key {
                    continue;
                }
                let start = offset_to_position(&file_content, decl.start);
                let end = offset_to_position(&file_content, decl.end);
                push_unique_location(&mut locations, &parsed_uri, start, end);
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
    let cursor_offset = crate::util::position_to_offset(content, position) as usize;
    let decls = collect_laravel_config_declarations(content, &prefix);
    let match_ = decls
        .into_iter()
        .find(|d| cursor_offset >= d.start && cursor_offset <= d.end)?;

    let target_uri = Url::parse(uri).ok()?;
    let pos = crate::util::offset_to_position(content, match_.start);
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
}
