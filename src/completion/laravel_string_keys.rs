//! Laravel string key completion.
//!
//! Offers autocompletion for route names, config keys, view names, and
//! translation keys inside their respective helper calls:
//!
//! - `route('|')` / `to_route('|')` → route names
//! - `config('|')` / `Config::get('|')` → config keys
//! - `view('|')` / `View::make('|')` → view names
//! - `__('|')` / `trans('|')` / `Lang::get('|')` → translation keys

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::LaravelStringKind;
use crate::util::position_to_offset;

// ─── Context ────────────────────────────────────────────────────────────────

struct LaravelStringKeyContext {
    kind: LaravelStringKind,
    prefix: String,
    /// Byte offset of the string content start (right after the opening quote).
    content_start_offset: usize,
    /// When set, the key is a sub-key under this config path prefix.
    /// For example, `#[Database('mysql')]` sets this to `"database.connections."`
    /// so completion filters to `database.connections.*` keys and strips the
    /// prefix, showing just `mysql`, `sqlite`, etc.
    config_sub_prefix: Option<&'static str>,
}

// ─── Detection ──────────────────────────────────────────────────────────────

/// Detect if the cursor is inside the first string argument of a Laravel
/// helper function.  Returns the key kind and the prefix typed so far.
fn detect_laravel_string_key_context(
    content: &str,
    position: Position,
) -> Option<LaravelStringKeyContext> {
    let cursor_offset = position_to_offset(content, position) as usize;
    let bytes = content.as_bytes();

    if cursor_offset == 0 || cursor_offset > bytes.len() {
        return None;
    }

    // ── Find the opening quote before the cursor ────────────────────
    let mut quote_pos = None;
    let mut i = cursor_offset;
    while i > 0 {
        i -= 1;
        let ch = bytes[i];
        if ch == b'\'' || ch == b'"' {
            let mut bs = 0;
            let mut j = i;
            while j > 0 && bytes[j - 1] == b'\\' {
                bs += 1;
                j -= 1;
            }
            if bs % 2 == 0 {
                quote_pos = Some(i);
                break;
            }
        }
        if ch == b'\n' {
            return None;
        }
    }
    let quote_pos = quote_pos?;
    let prefix = content[quote_pos + 1..cursor_offset].to_string();

    // ── Before the quote, expect `(` (first argument) ───────────────
    let before_quote = content[..quote_pos].trim_end();
    if !before_quote.ends_with('(') {
        return None;
    }
    let before_paren = before_quote[..before_quote.len() - 1].trim_end();

    // ── Extract the function/method name ────────────────────────────
    let bp_bytes = before_paren.as_bytes();
    let name_end = bp_bytes.len();
    let mut name_start = name_end;
    while name_start > 0
        && (bp_bytes[name_start - 1].is_ascii_alphanumeric() || bp_bytes[name_start - 1] == b'_')
    {
        name_start -= 1;
    }
    if name_start == name_end {
        return None;
    }
    let func_name = &before_paren[name_start..name_end];

    // ── Check for static method syntax (Config::get, etc.) ──────────
    let before_name = &before_paren[..name_start];
    let is_static = before_name.trim_end().ends_with("::");

    // Check for instance method call (->route() or ?->route())
    let trimmed_before = before_name.trim_end();
    let is_instance_method = trimmed_before.ends_with("->") || trimmed_before.ends_with("?->");

    // Check for PHP attribute syntax: #[Config('key')] or
    // #[\Illuminate\Container\Attributes\Config('key')].
    // Strip trailing `\Identifier` segments to handle FQN attributes,
    // then check for `#[`.  Never search the entire file prefix —
    // an unrelated attribute (e.g. `#[Override]`) would false-positive.
    let is_attribute = {
        let mut s = trimmed_before;
        loop {
            let stripped = s.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
            if stripped.len() < s.len() && stripped.ends_with('\\') {
                s = &stripped[..stripped.len() - 1];
            } else {
                s = stripped;
                break;
            }
        }
        s.ends_with("#[") || s.ends_with("#")
    };

    // ── Map container attributes to config sub-prefixes ────────────
    let (kind, config_sub_prefix) = if is_attribute {
        // Resolve the attribute to its Laravel FQN.  When the name is
        // fully qualified (contains `\`), match the FQN directly.
        // When it's a short name, verify the file imports it from
        // `Illuminate\Container\Attributes\`.
        const ATTR_NS: &str = "Illuminate\\Container\\Attributes\\";

        // Reconstruct the full attribute class name by scanning backwards
        // past namespace separators.  `func_name` only captured the last
        // segment (e.g. `Config`), but the FQN parts (if any) are in
        // `before_name` (e.g. `#[\Illuminate\Container\Attributes\`).
        let full_attr_name = {
            let bn = before_name.trim_end().trim_end_matches('\\');
            // Check for `#[` or `#[\` prefix — extract everything after `#[`
            if let Some(idx) = bn.rfind("#[") {
                let after_hash = &bn[idx + 2..].trim_start_matches('\\');
                if after_hash.is_empty() {
                    func_name.to_string()
                } else {
                    format!("{}\\{}", after_hash, func_name)
                }
            } else {
                func_name.to_string()
            }
        };
        let attr_class = full_attr_name.trim_start_matches('\\');
        let short = attr_class.rsplit('\\').next().unwrap_or(attr_class);

        let is_fqn = attr_class.contains('\\');
        let fqn_matches = |expected_short: &str| -> bool {
            if is_fqn {
                attr_class == format!("{}{}", ATTR_NS, expected_short)
            } else if short == expected_short {
                // Verify the import exists in the file.
                content.contains(&format!("use {}{};", ATTR_NS, expected_short))
                    || content.contains(&format!("use {}{{", ATTR_NS))
            } else {
                false
            }
        };

        if fqn_matches("Config") {
            (Some(LaravelStringKind::Config), None)
        } else if fqn_matches("Database") || fqn_matches("DB") {
            (
                Some(LaravelStringKind::Config),
                Some("database.connections."),
            )
        } else if fqn_matches("Cache") {
            (Some(LaravelStringKind::Config), Some("cache.stores."))
        } else if fqn_matches("Log") {
            (Some(LaravelStringKind::Config), Some("logging.channels."))
        } else if fqn_matches("Storage") {
            (Some(LaravelStringKind::Config), Some("filesystems.disks."))
        } else if fqn_matches("Auth") || fqn_matches("Authenticated") {
            (Some(LaravelStringKind::Config), Some("auth.guards."))
        } else {
            (None, None)
        }
    } else if is_static {
        let before_colons = &trimmed_before[..trimmed_before.len() - 2].trim_end();
        let bc_bytes = before_colons.as_bytes();
        let mut cls_start = bc_bytes.len();
        while cls_start > 0
            && (bc_bytes[cls_start - 1].is_ascii_alphanumeric()
                || bc_bytes[cls_start - 1] == b'_'
                || bc_bytes[cls_start - 1] == b'\\')
        {
            cls_start -= 1;
        }
        let class_name = &before_colons[cls_start..];
        let short = class_name.rsplit('\\').next().unwrap_or(class_name);
        let fn_lower = func_name.to_ascii_lowercase();

        match (short.to_ascii_lowercase().as_str(), fn_lower.as_str()) {
            (
                "config",
                "get" | "set" | "has" | "boolean" | "array" | "collection" | "prepend" | "push",
            ) => (Some(LaravelStringKind::Config), None),
            ("view", "make" | "exists") => (Some(LaravelStringKind::View), None),
            ("lang", "get" | "has" | "choice") => (Some(LaravelStringKind::Trans), None),
            // Facade methods that accept config sub-keys:
            ("auth", "guard") => (Some(LaravelStringKind::Config), Some("auth.guards.")),
            ("db", "connection") => (
                Some(LaravelStringKind::Config),
                Some("database.connections."),
            ),
            ("cache", "store") => (Some(LaravelStringKind::Config), Some("cache.stores.")),
            ("log", "channel") => (Some(LaravelStringKind::Config), Some("logging.channels.")),
            ("storage", "disk") => (Some(LaravelStringKind::Config), Some("filesystems.disks.")),
            _ => (None, None),
        }
    } else if is_instance_method {
        let k = match func_name.to_ascii_lowercase().as_str() {
            "route" => Some(LaravelStringKind::Route),
            _ => None,
        };
        (k, None)
    } else {
        match func_name.to_ascii_lowercase().as_str() {
            "route" | "to_route" => (Some(LaravelStringKind::Route), None),
            "config" => (Some(LaravelStringKind::Config), None),
            "view" | "blade_view_directive" => (Some(LaravelStringKind::View), None),
            "__" | "trans" | "trans_choice" => (Some(LaravelStringKind::Trans), None),
            // auth('guard') helper accepts a guard name
            "auth" => (Some(LaravelStringKind::Config), Some("auth.guards.")),
            _ => (None, None),
        }
    };

    let kind = kind?;

    Some(LaravelStringKeyContext {
        kind,
        prefix,
        content_start_offset: quote_pos + 1,
        config_sub_prefix,
    })
}

// ─── Enumeration ────────────────────────────────────────────────────────────

impl Backend {
    /// Enumerate all view names by scanning `resources/views/` file URIs.
    fn enumerate_all_view_names(&self) -> Vec<String> {
        let snapshot = self.user_file_symbol_maps();
        let mut names = Vec::new();

        for (file_uri, _) in snapshot {
            let Some(rel) = extract_view_relative_path(&file_uri) else {
                continue;
            };
            names.push(rel);
        }

        for res in &self.laravel_provider_resources.read().view_dirs {
            collect_namespaced_view_names(&res.path, &res.namespace, &mut names);
        }

        names.sort();
        names.dedup();
        names
    }

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

        keys.sort();
        keys.dedup();
        keys
    }

    /// Enumerate all translation keys by scanning `lang/` files and
    /// package translation directories discovered from service providers.
    ///
    /// Supports both PHP array files (`lang/en/messages.php` → `messages.key`)
    /// and JSON translation files (`lang/en.json` → raw key strings).
    /// Package translations use `namespace::file.key` syntax.
    fn enumerate_all_trans_keys(&self) -> Vec<String> {
        let snapshot = self.user_file_symbol_maps();
        let mut keys = Vec::new();

        for (file_uri, _) in &snapshot {
            if !(file_uri.contains("/lang/") || file_uri.contains("/resources/lang/")) {
                continue;
            }
            if !file_uri.ends_with(".php") {
                continue;
            }
            let Some(stem) = extract_lang_file_stem(file_uri) else {
                continue;
            };
            let Some(content) = self.get_file_content(file_uri) else {
                continue;
            };
            let decls =
                crate::virtual_members::laravel::collect_trans_declarations(&content, &stem);
            for d in decls {
                keys.push(d.key);
            }
        }

        collect_json_trans_keys(self, &mut keys);

        for res in &self.laravel_provider_resources.read().trans_dirs {
            collect_namespaced_trans_keys(&res.path, &res.namespace, &mut keys);
        }

        keys.sort();
        keys.dedup();
        keys
    }

    pub(crate) fn cached_route_names(&self) -> Vec<String> {
        {
            let cache = self.laravel_string_key_cache.read();
            if let Some(ref names) = cache.route_names {
                return names.clone();
            }
        }
        let names = crate::virtual_members::laravel::enumerate_all_route_names(self);
        self.laravel_string_key_cache.write().route_names = Some(names.clone());
        names
    }

    pub(crate) fn cached_config_keys(&self) -> Vec<String> {
        {
            let cache = self.laravel_string_key_cache.read();
            if let Some(ref keys) = cache.config_keys {
                return keys.clone();
            }
        }
        let keys = self.enumerate_all_config_keys();
        self.laravel_string_key_cache.write().config_keys = Some(keys.clone());
        keys
    }

    pub(crate) fn cached_view_names(&self) -> Vec<String> {
        {
            let cache = self.laravel_string_key_cache.read();
            if let Some(ref names) = cache.view_names {
                return names.clone();
            }
        }
        let names = self.enumerate_all_view_names();
        self.laravel_string_key_cache.write().view_names = Some(names.clone());
        names
    }

    pub(crate) fn cached_trans_keys(&self) -> Vec<String> {
        {
            let cache = self.laravel_string_key_cache.read();
            if let Some(ref keys) = cache.trans_keys {
                return keys.clone();
            }
        }
        let keys = self.enumerate_all_trans_keys();
        self.laravel_string_key_cache.write().trans_keys = Some(keys.clone());
        keys
    }
}

/// Extract the dot-notated view name from a file URI.
///
/// `file:///path/resources/views/users/profile.blade.php` → `"users.profile"`
pub(crate) fn extract_view_relative_path(uri: &str) -> Option<String> {
    let marker = "/resources/views/";
    let idx = uri.find(marker)?;
    let rel = &uri[idx + marker.len()..];
    let name = rel
        .strip_suffix(".blade.php")
        .or_else(|| rel.strip_suffix(".php"))?;
    if name.is_empty() {
        return None;
    }
    Some(name.replace('/', "."))
}

/// Extract the file stem from a lang file URI for use as the translation
/// key prefix.
///
/// `file:///path/lang/en/messages.php` → `"messages"`
fn extract_lang_file_stem(uri: &str) -> Option<String> {
    let file = uri.rsplit('/').next()?;
    let stem = file.strip_suffix(".php")?;
    if stem.is_empty() {
        return None;
    }
    Some(stem.to_string())
}

/// Scan the workspace for `lang/*.json` files and collect their top-level
/// keys into `out`.  Laravel's JSON translations are flat
/// `{ "Some phrase": "Translated phrase" }` objects where the key is used
/// directly in `__('Some phrase')`.
///
/// We scan the filesystem because JSON files are not PHP and therefore do
/// not appear in `user_file_symbol_maps()`.
fn collect_json_trans_keys(backend: &crate::Backend, out: &mut Vec<String>) {
    let root = match backend.workspace_root.read().clone() {
        Some(r) => r,
        None => return,
    };
    for sub in &["lang", "resources/lang"] {
        let dir = root.join(sub);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(content) = std::fs::read_to_string(&path)
                && let Ok(map) =
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content)
            {
                for k in map.keys() {
                    out.push(k.clone());
                }
            }
        }
    }
}

/// Recursively scan a package view directory and collect view names
/// in `namespace::dot.notation` format.
fn collect_namespaced_view_names(dir: &std::path::Path, namespace: &str, out: &mut Vec<String>) {
    collect_view_names_recursive(dir, dir, namespace, out);
}

fn collect_view_names_recursive(
    base: &std::path::Path,
    dir: &std::path::Path,
    namespace: &str,
    out: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_view_names_recursive(base, &path, namespace, out);
        } else if let Some(rel) = path.strip_prefix(base).ok().and_then(|r| r.to_str()) {
            let name = rel
                .strip_suffix(".blade.php")
                .or_else(|| rel.strip_suffix(".php"));
            if let Some(name) = name {
                let dotted = name.replace([std::path::MAIN_SEPARATOR, '/'], ".");
                out.push(format!("{namespace}::{dotted}"));
            }
        }
    }
}

/// Scan a package translation directory and collect keys in
/// `namespace::file.key` format (PHP files) or `namespace::raw_key`
/// (JSON files with empty namespace).
fn collect_namespaced_trans_keys(dir: &std::path::Path, namespace: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_namespaced_trans_from_locale_dir(&path, namespace, out);
        } else if path.extension().is_some_and(|e| e == "json")
            && namespace.is_empty()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(map) =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content)
        {
            for k in map.keys() {
                out.push(k.clone());
            }
        }
    }
}

fn collect_namespaced_trans_from_locale_dir(
    dir: &std::path::Path,
    namespace: &str,
    out: &mut Vec<String>,
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
            out.push(d.key);
        }
    }
}

// ─── Completion ─────────────────────────────────────────────────────────────

impl Backend {
    /// Try Laravel string key completion.
    ///
    /// Detects the cursor inside the first string argument of `route()`,
    /// `config()`, `view()`, `__()`, etc. and offers matching key names.
    pub(crate) fn try_laravel_string_key_completion(
        &self,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let ctx = detect_laravel_string_key_context(content, position)?;

        let mut candidates = match ctx.kind {
            LaravelStringKind::Route => self.cached_route_names(),
            LaravelStringKind::Config => self.cached_config_keys(),
            LaravelStringKind::View => self.cached_view_names(),
            LaravelStringKind::Trans => self.cached_trans_keys(),
        };

        // For config-backed attributes like #[Database('mysql')], filter
        // to sub-keys under the relevant config prefix and strip it so
        // the user sees just the connection/store/channel name.
        if let Some(sub_prefix) = ctx.config_sub_prefix {
            candidates = candidates
                .into_iter()
                .filter_map(|key| {
                    key.strip_prefix(sub_prefix).and_then(|rest| {
                        // Only show direct children (no dots = leaf key).
                        if rest.contains('.') {
                            None
                        } else {
                            Some(rest.to_string())
                        }
                    })
                })
                .collect();
            candidates.sort();
            candidates.dedup();
        }

        // Build the TextEdit range: from the start of the string content
        // (right after the opening quote) to the current cursor position.
        // This replaces the entire typed prefix with the selected name,
        // so dots in the name don't break the editor's word-based filter.
        let start_pos = crate::util::offset_to_position(content, ctx.content_start_offset);
        let edit_range = Range {
            start: start_pos,
            end: position,
        };

        let prefix_lower = ctx.prefix.to_lowercase();
        let items: Vec<CompletionItem> = candidates
            .into_iter()
            .filter(|name| {
                if prefix_lower.is_empty() {
                    true
                } else {
                    name.to_lowercase().starts_with(&prefix_lower)
                }
            })
            .enumerate()
            .map(|(i, name)| {
                let kind = match ctx.kind {
                    LaravelStringKind::Route => CompletionItemKind::VALUE,
                    LaravelStringKind::Config => CompletionItemKind::PROPERTY,
                    LaravelStringKind::View => CompletionItemKind::FILE,
                    LaravelStringKind::Trans => CompletionItemKind::TEXT,
                };
                CompletionItem {
                    label: name.clone(),
                    kind: Some(kind),
                    sort_text: Some(format!("{:05}", i)),
                    filter_text: Some(name.clone()),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range: edit_range,
                        new_text: name,
                    })),
                    ..Default::default()
                }
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn detects_route_call() {
        let content = "<?php\nroute('user.');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("user.").unwrap() as u32 + 5;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect route() context");
        assert!(matches!(ctx.kind, LaravelStringKind::Route));
        assert_eq!(ctx.prefix, "user.");
    }

    #[test]
    fn detects_to_route_call() {
        let content = "<?php\nto_route('home');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("home").unwrap() as u32 + 2;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect to_route() context");
        assert!(matches!(ctx.kind, LaravelStringKind::Route));
        assert_eq!(ctx.prefix, "ho");
    }

    #[test]
    fn detects_config_call() {
        let content = "<?php\nconfig('app.');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("app.").unwrap() as u32 + 4;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect config() context");
        assert!(matches!(ctx.kind, LaravelStringKind::Config));
        assert_eq!(ctx.prefix, "app.");
    }

    #[test]
    fn detects_config_static_get() {
        let content = "<?php\nConfig::get('app.name');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("app.").unwrap() as u32 + 4;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect Config::get() context");
        assert!(matches!(ctx.kind, LaravelStringKind::Config));
        assert_eq!(ctx.prefix, "app.");
    }

    #[test]
    fn detects_view_call() {
        let content = "<?php\nview('users.');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("users.").unwrap() as u32 + 6;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect view() context");
        assert!(matches!(ctx.kind, LaravelStringKind::View));
    }

    #[test]
    fn detects_trans_double_underscore() {
        let content = "<?php\n__('messages.');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("messages.").unwrap() as u32 + 9;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect __() context");
        assert!(matches!(ctx.kind, LaravelStringKind::Trans));
    }

    #[test]
    fn detects_empty_prefix() {
        let content = "<?php\nroute('');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("''").unwrap() as u32 + 1;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect empty prefix");
        assert!(matches!(ctx.kind, LaravelStringKind::Route));
        assert_eq!(ctx.prefix, "");
    }

    #[test]
    fn rejects_second_arg() {
        let content = "<?php\nroute('name', 'param');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("param").unwrap() as u32 + 2;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        assert!(ctx.is_none(), "Second argument should not match");
    }

    #[test]
    fn rejects_non_laravel_function() {
        let content = "<?php\nfoo('bar');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("bar").unwrap() as u32 + 1;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        assert!(ctx.is_none(), "Non-Laravel function should not match");
    }

    #[test]
    fn view_relative_path_extraction() {
        assert_eq!(
            extract_view_relative_path("file:///app/resources/views/users/profile.blade.php"),
            Some("users.profile".to_string())
        );
        assert_eq!(
            extract_view_relative_path("file:///app/resources/views/home.blade.php"),
            Some("home".to_string())
        );
        assert_eq!(
            extract_view_relative_path("file:///app/src/Controller.php"),
            None
        );
    }

    #[test]
    fn lang_file_stem_extraction() {
        assert_eq!(
            extract_lang_file_stem("file:///app/lang/en/messages.php"),
            Some("messages".to_string())
        );
        assert_eq!(
            extract_lang_file_stem("file:///app/resources/lang/en/validation.php"),
            Some("validation".to_string())
        );
    }

    #[test]
    fn detects_config_attribute_with_import() {
        let content =
            "<?php\nuse Illuminate\\Container\\Attributes\\Config;\n#[Config('app.timezone')]\n";
        let line = 2;
        let line_text = content.lines().nth(2).unwrap();
        let col = line_text.find("app.timezone").unwrap() as u32 + 12;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect #[Config()] with verified import");
        assert!(matches!(ctx.kind, LaravelStringKind::Config));
        assert_eq!(ctx.prefix, "app.timezone");
    }

    #[test]
    fn rejects_config_attribute_without_import() {
        let content = "<?php\n#[Config('app.timezone')]\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("app.timezone").unwrap() as u32 + 12;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        assert!(
            ctx.is_none(),
            "Should reject #[Config()] without verified import"
        );
    }

    #[test]
    fn unrelated_attribute_does_not_break_detection() {
        let content = "<?php\nclass Foo {\n    #[Override]\n    public function bar(): void {\n        route('');\n    }\n}\n";
        let line = 4;
        let line_text = content.lines().nth(line).unwrap();
        let col = line_text.find("''").unwrap() as u32 + 1;
        let ctx = detect_laravel_string_key_context(content, Position::new(line as u32, col));
        assert!(
            ctx.is_some(),
            "route('') must be detected even when #[Override] exists earlier in the file"
        );
        assert!(matches!(ctx.unwrap().kind, LaravelStringKind::Route));
    }

    #[test]
    fn detects_fqn_config_attribute() {
        let content = "<?php\n#[\\Illuminate\\Container\\Attributes\\Config('app.')]\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("app.").unwrap() as u32 + 4;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect FQN #[Config()] attribute");
        assert!(matches!(ctx.kind, LaravelStringKind::Config));
        assert_eq!(ctx.prefix, "app.");
    }

    #[test]
    fn detects_route_in_module_controller() {
        let content = "<?php\n\
\n\
namespace Acme\\User\\Http\\Controllers;\n\
\n\
use App\\Http\\Controllers\\Abstracts\\BaseController;\n\
use Illuminate\\Http\\RedirectResponse;\n\
use Illuminate\\Http\\Request;\n\
\n\
final class UserPermissionController extends BaseController\n\
{\n\
    public function copy(Request $request): RedirectResponse\n\
    {\n\
        route('');\n\
\n\
        return to_route('admin::user.permissions.edit', 1);\n\
    }\n\
}\n";
        // route('') is on line 12 (0-indexed), cursor at character 15 (between quotes)
        let line = content
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("route('')"))
            .map(|(i, _)| i as u32)
            .expect("should find route('') line");
        let line_text = content.lines().nth(line as usize).unwrap();
        let col = line_text.find("''").unwrap() as u32 + 1;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        assert!(
            ctx.is_some(),
            "should detect route('') context in module controller at line {}, col {}",
            line,
            col,
        );
        let ctx = ctx.unwrap();
        assert!(matches!(ctx.kind, LaravelStringKind::Route));
    }

    #[test]
    fn route_completion_end_to_end() {
        let backend = crate::Backend::new_test();

        let route_uri = "file:///app/routes/web.php";
        let route_content = "<?php\n\
            use Illuminate\\Support\\Facades\\Route;\n\
            Route::get('/home', fn() => 'home')->name('home');\n\
            Route::get('/about', fn() => 'about')->name('about');\n";
        backend.open_files.write().insert(
            route_uri.to_string(),
            std::sync::Arc::new(route_content.to_string()),
        );
        backend.update_ast(route_uri, route_content);

        let test_content = "<?php\nroute('');\n";
        backend.update_ast("file:///app/Http/Controllers/Test.php", test_content);

        let names = backend.cached_route_names();
        assert!(
            names.contains(&"home".to_string()),
            "cached_route_names should contain 'home', got: {:?}",
            names
        );

        let response = backend.try_laravel_string_key_completion(test_content, Position::new(1, 7));
        assert!(
            response.is_some(),
            "try_laravel_string_key_completion should return Some for route('')"
        );
        if let Some(CompletionResponse::Array(items)) = response {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.contains(&"home"),
                "completion should include 'home', got: {:?}",
                labels
            );
            assert!(
                labels.contains(&"about"),
                "completion should include 'about', got: {:?}",
                labels
            );
        }
    }
}
