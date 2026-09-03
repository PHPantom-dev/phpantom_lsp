//! Laravel string key completion.
//!
//! Offers autocompletion for the keys a Laravel call site names, inside
//! whichever helper, facade method, or attribute names them:
//!
//! - `route('|')` / `URL::signedRoute('|')` / `Route::is('|')` → route names
//! - `config('|')` / `Config::get('|')` → config keys
//! - `view('|')` / `View::make('|')` → view names
//! - `__('|')` / `trans('|')` / `Lang::get('|')` → translation keys
//! - `env('|')` / `Env::get('|')` → environment variables
//!
//! Detection is textual rather than AST-based: it runs on a buffer that is
//! mid-edit, where the call being typed usually does not parse yet.  The
//! symbol map decides the same question for a *complete* file, and the two
//! are kept in step by hand.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::LaravelStringKind;
use crate::text_position::position_to_offset;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum StringArgumentShape {
    Scalar,
    ArrayValue,
}

struct StringArgumentContext<'a> {
    callable: &'a str,
    named_argument: Option<&'a str>,
    shape: StringArgumentShape,
}

#[inline]
fn is_unescaped(bytes: &[u8], index: usize) -> bool {
    let mut before = index;
    while before > 0 && bytes[before - 1] == b'\\' {
        before -= 1;
    }
    (index - before).is_multiple_of(2)
}

/// Find the unmatched call parenthesis enclosing a named argument.
fn enclosing_call_open_paren(content: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut quote = None;
    let mut index = bytes.len();

    while index > 0 {
        index -= 1;
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote && is_unescaped(bytes, index) {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b')' => parens += 1,
            b'(' if parens > 0 => parens -= 1,
            b'(' if brackets == 0 && braces == 0 => return Some(index),
            b']' => brackets += 1,
            b'[' if brackets > 0 => brackets -= 1,
            b'}' => braces += 1,
            b'{' if braces > 0 => braces -= 1,
            b';' if parens == 0 && brackets == 0 && braces == 0 => return None,
            _ => {}
        }
    }

    None
}

/// Return the callable before a scalar first argument or a named argument.
fn callable_before_scalar_argument(before_value: &str) -> Option<(&str, Option<&str>)> {
    let before_value = before_value.trim_end();
    if let Some(callable) = before_value.strip_suffix('(') {
        return Some((callable.trim_end(), None));
    }

    let colon = before_value.rfind(':')?;
    if !before_value[colon + 1..].trim().is_empty() {
        return None;
    }
    let before_label = before_value[..colon].trim_end();
    let label_start = before_label
        .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .map_or(0, |index| index + 1);
    if label_start == before_label.len() {
        return None;
    }
    let argument = &before_label[label_start..];
    let before_argument = before_label[..label_start].trim_end();
    let open_paren = enclosing_call_open_paren(before_argument)?;
    Some((before_argument[..open_paren].trim_end(), Some(argument)))
}

/// Return the callable owning an array that directly contains this literal.
fn callable_before_array_argument(before_quote: &str) -> Option<(&str, Option<&str>)> {
    let bytes = before_quote.as_bytes();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut string_quote = None;
    let mut index = bytes.len();

    while index > 0 {
        index -= 1;
        let byte = bytes[index];
        if let Some(quote) = string_quote {
            if byte == quote && is_unescaped(bytes, index) {
                string_quote = None;
            }
            continue;
        }

        match byte {
            b'\'' | b'"' => string_quote = Some(byte),
            b']' if paren_depth == 0 && brace_depth == 0 => bracket_depth += 1,
            b'[' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                return callable_before_scalar_argument(before_quote[..index].trim_end());
            }
            b'[' if paren_depth == 0 && brace_depth == 0 => bracket_depth -= 1,
            b')' => paren_depth += 1,
            b'(' if paren_depth > 0 => paren_depth -= 1,
            b'(' if bracket_depth == 0 && brace_depth == 0 => {
                let before_open = before_quote[..index].trim_end();
                let mut token_start = before_open.len();
                let token_bytes = before_open.as_bytes();
                while token_start > 0
                    && (token_bytes[token_start - 1].is_ascii_alphanumeric()
                        || token_bytes[token_start - 1] == b'_')
                {
                    token_start -= 1;
                }
                if before_open[token_start..].eq_ignore_ascii_case("array") {
                    return callable_before_scalar_argument(before_open[..token_start].trim_end());
                }
                return None;
            }
            b'}' => brace_depth += 1,
            b'{' if brace_depth > 0 => brace_depth -= 1,
            b'{' if bracket_depth == 0 && paren_depth == 0 => return None,
            b';' if bracket_depth == 0 && paren_depth == 0 && brace_depth == 0 => return None,
            _ => {}
        }
    }

    None
}

/// Resource arrays name values; an associative key is bookkeeping, not a disk.
fn string_literal_is_array_key(content: &str, cursor: usize, quote: u8) -> bool {
    let bytes = content.as_bytes();
    let mut index = cursor;
    while index < bytes.len() {
        if bytes[index] == quote && is_unescaped(bytes, index) {
            return content[index + 1..].trim_start().starts_with("=>");
        }
        if bytes[index] == b'\n' {
            return false;
        }
        index += 1;
    }
    false
}

fn string_argument_context<'a>(
    content: &'a str,
    before_quote: &'a str,
    cursor: usize,
    quote: u8,
) -> Option<StringArgumentContext<'a>> {
    if let Some((callable, named_argument)) = callable_before_array_argument(before_quote) {
        if string_literal_is_array_key(content, cursor, quote) {
            return None;
        }
        return Some(StringArgumentContext {
            callable,
            named_argument,
            shape: StringArgumentShape::ArrayValue,
        });
    }

    let (callable, named_argument) = callable_before_scalar_argument(before_quote)?;
    Some(StringArgumentContext {
        callable,
        named_argument,
        shape: StringArgumentShape::Scalar,
    })
}

fn imported_item_is_storage(
    item: &str,
    group_prefix: Option<&str>,
    expected_fqn: &str,
    referenced_name: &str,
) -> Option<bool> {
    let mut words = item.split_whitespace();
    let imported_name = words.next()?;
    let alias = match words.next() {
        Some(as_keyword) if as_keyword.eq_ignore_ascii_case("as") => Some(words.next()?),
        Some(_) => return None,
        None => None,
    };
    if words.next().is_some() {
        return None;
    }

    let imported_name = imported_name.trim_start_matches('\\');
    let class_matches = if let Some(prefix) = group_prefix {
        let expected_prefix = expected_fqn
            .strip_suffix("Storage")
            .unwrap_or(expected_fqn)
            .trim_end_matches('\\');
        prefix
            .trim_start_matches('\\')
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(expected_prefix)
            && imported_name.eq_ignore_ascii_case("Storage")
    } else {
        imported_name.eq_ignore_ascii_case(expected_fqn)
    };
    let local_name =
        alias.unwrap_or_else(|| imported_name.rsplit('\\').next().unwrap_or(imported_name));
    local_name
        .eq_ignore_ascii_case(referenced_name)
        .then_some(class_matches)
}

/// Whether a class spelling resolves to Laravel's Storage facade.
fn is_storage_facade_reference(content: &str, class_name: &str) -> bool {
    const STORAGE_FACADE: &str = "Illuminate\\Support\\Facades\\Storage";

    let is_root_qualified = class_name.starts_with('\\');
    let class_name = class_name.trim_start_matches('\\');
    if class_name.contains('\\') {
        return class_name.eq_ignore_ascii_case(STORAGE_FACADE);
    }

    for statement in content.split(';') {
        let mut line_offset = 0usize;
        let mut clause = None;
        for line in statement.split_inclusive('\n') {
            let trimmed = line.trim_start();
            if trimmed
                .get(..4)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("use "))
            {
                let leading = line.len() - trimmed.len();
                clause = Some(statement[line_offset + leading + 4..].trim());
                break;
            }
            line_offset += line.len();
        }
        let Some(clause) = clause else {
            continue;
        };

        if let Some(open) = clause.find('{') {
            let Some(close) = clause.rfind('}') else {
                continue;
            };
            let prefix = clause[..open].trim();
            if let Some(matches) = clause[open + 1..close].split(',').find_map(|item| {
                imported_item_is_storage(item, Some(prefix), STORAGE_FACADE, class_name)
            }) {
                return matches;
            }
        } else if let Some(matches) = clause
            .split(',')
            .find_map(|item| imported_item_is_storage(item, None, STORAGE_FACADE, class_name))
        {
            return matches;
        }
    }

    class_name.eq_ignore_ascii_case("Storage")
        && (is_root_qualified || !source_has_namespace(content))
}

fn source_has_namespace(content: &str) -> bool {
    content.lines().any(|line| {
        let mut line = line.trim_start();
        if let Some(rest) = line.strip_prefix("<?php") {
            line = rest.trim_start();
        }
        line.get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("namespace"))
            && line.as_bytes().get(9).is_some_and(u8::is_ascii_whitespace)
    })
}

// ─── Detection ──────────────────────────────────────────────────────────────

/// Detect if the cursor is inside a supported string argument of a Laravel
/// helper or facade call. Returns the key kind and the prefix typed so far.
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
        if (ch == b'\'' || ch == b'"') && is_unescaped(bytes, i) {
            quote_pos = Some(i);
            break;
        }
        if ch == b'\n' {
            return None;
        }
    }
    let quote_pos = quote_pos?;
    let prefix = content[quote_pos + 1..cursor_offset].to_string();

    // ── Locate the call argument that owns this string ─────────────
    let before_quote = content[..quote_pos].trim_end();
    let argument = string_argument_context(content, before_quote, cursor_offset, bytes[quote_pos])?;
    let before_paren = argument.callable;

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
    // Everything between the nearest `#[` and this final class-name segment
    // must itself be a class-name prefix. This recognizes FQN attributes
    // without letting an unrelated attribute earlier in the file match.
    let is_attribute = trimmed_before.rfind("#[").is_some_and(|start| {
        trimmed_before[start + 2..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\\')
    });

    // ── Map container attributes to config sub-prefixes ────────────
    let (kind, config_sub_prefix) = if is_attribute {
        if argument.shape != StringArgumentShape::Scalar {
            return None;
        }
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

        // The namespace holding `#[RedirectToRoute]`, which names a route
        // rather than a config key.
        const HTTP_ATTR_NS: &str = "Illuminate\\Foundation\\Http\\Attributes\\";

        let is_fqn = attr_class.contains('\\');
        let attr_matches = |ns: &str, expected_short: &str| -> bool {
            if is_fqn {
                attr_class == format!("{}{}", ns, expected_short)
            } else if short == expected_short {
                // Verify the import exists in the file.
                content.contains(&format!("use {}{};", ns, expected_short))
                    || content.contains(&format!("use {}{{", ns))
            } else {
                false
            }
        };
        let fqn_matches = |expected_short: &str| attr_matches(ATTR_NS, expected_short);

        if argument.named_argument.is_some()
            && (!fqn_matches("Storage")
                || argument
                    .named_argument
                    .is_some_and(|name| !name.eq_ignore_ascii_case("disk")))
        {
            return None;
        }

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
        } else if attr_matches(HTTP_ATTR_NS, "RedirectToRoute") {
            (Some(LaravelStringKind::Route), None)
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

        let storage_argument = if is_storage_facade_reference(content, class_name) {
            if func_name.eq_ignore_ascii_case("disk") {
                Some(("name", false))
            } else if func_name.eq_ignore_ascii_case("fake")
                || func_name.eq_ignore_ascii_case("persistentFake")
            {
                Some(("disk", false))
            } else if func_name.eq_ignore_ascii_case("forgetDisk") {
                Some(("disk", true))
            } else {
                None
            }
        } else {
            None
        };
        let fn_lower = func_name.to_ascii_lowercase();
        let short_lower = short.to_ascii_lowercase();
        let accepts_array = matches!(
            (short_lower.as_str(), fn_lower.as_str()),
            ("config", "getmany") | ("route", "is" | "currentroutenamed")
        );

        if let Some((expected_name, accepts_array)) = storage_argument {
            if argument
                .named_argument
                .is_some_and(|name| !name.eq_ignore_ascii_case(expected_name))
                || (argument.shape == StringArgumentShape::ArrayValue && !accepts_array)
            {
                return None;
            }
            (Some(LaravelStringKind::Config), Some("filesystems.disks."))
        } else if argument.named_argument.is_some()
            || (argument.shape != StringArgumentShape::Scalar
                && (!accepts_array || !before_quote.trim_end().ends_with('[')))
        {
            (None, None)
        } else {
            match (short_lower.as_str(), fn_lower.as_str()) {
                (
                    "config",
                    "get" | "getmany" | "set" | "has" | "boolean" | "array" | "collection"
                    | "prepend" | "push",
                ) => (Some(LaravelStringKind::Config), None),
                ("view", "make" | "exists") => (Some(LaravelStringKind::View), None),
                ("lang", "get" | "has" | "hasforlocale" | "choice") => {
                    (Some(LaravelStringKind::Trans), None)
                }
                // Route names reached through the URL-building facades, and the
                // "is the current route named …?" predicates.
                (
                    "url" | "redirect" | "response",
                    "route" | "signedroute" | "temporarysignedroute" | "redirecttoroute",
                ) => (Some(LaravelStringKind::Route), None),
                ("route", "is" | "currentroutenamed") => (Some(LaravelStringKind::Route), None),
                ("env", "get" | "getorfail") => (Some(LaravelStringKind::Env), None),
                // Facade methods that accept config sub-keys:
                ("auth", "guard") => (Some(LaravelStringKind::Config), Some("auth.guards.")),
                ("db", "connection") => (
                    Some(LaravelStringKind::Config),
                    Some("database.connections."),
                ),
                ("cache", "store") => (Some(LaravelStringKind::Config), Some("cache.stores.")),
                ("log", "channel") => (Some(LaravelStringKind::Config), Some("logging.channels.")),
                // Artisan command names.
                ("artisan", "call" | "queue") => (Some(LaravelStringKind::Command), None),
                ("schedule", "command") => (Some(LaravelStringKind::Command), None),
                // Eloquent morph aliases.
                ("relation", "getmorphedmodel") => (Some(LaravelStringKind::MorphAlias), None),
                ("model", "getactualclassnameformorph") => {
                    (Some(LaravelStringKind::MorphAlias), None)
                }
                // Authorization abilities checked through the Gate facade.
                (
                    "gate",
                    "allows" | "denies" | "check" | "any" | "none" | "authorize" | "inspect"
                    | "has" | "define",
                ) => (Some(LaravelStringKind::GateAbility), None),
                _ => (None, None),
            }
        }
    } else if is_instance_method {
        if argument.named_argument.is_some() || argument.shape != StringArgumentShape::Scalar {
            return None;
        }
        // Whether the receiver is `$this` (used to scope command-running
        // methods, whose names are too generic to match on any object).
        let receiver_is_this = {
            let recv = trimmed_before
                .trim_end_matches("?->")
                .trim_end_matches("->")
                .trim_end();
            recv.ends_with("$this")
        };
        // Whether the receiver plainly reads as the authenticated user,
        // which is what makes `->can('…')` an authorization check rather
        // than a same-named method on an unrelated object.  Mirrors the
        // symbol-map rule that decides which `can()` calls get a span.
        let receiver_is_user_like = {
            let recv = trimmed_before
                .trim_end_matches("?->")
                .trim_end_matches("->")
                .trim_end();
            let tail = recv
                .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            tail.to_ascii_lowercase().ends_with("user") || recv.ends_with("user()")
        };
        // A chain that starts at the `Gate` facade
        // (`Gate::forUser($user)->allows('…')`) or at a route registration
        // (`Route::get(…)->can('…')`) is an authorization check whatever the
        // rest of the chain looks like.  Only the text back to the start of
        // the statement is searched — `trimmed_before` is the whole file
        // prefix, and an unrelated `Gate::` far above would false-positive.
        let chain_text = &trimmed_before[trimmed_before
            .rfind(['\n', ';', '{', '}'])
            .map_or(0, |idx| idx + 1)..];
        let chain_starts_at_gate = chain_text.contains("Gate::");
        let chain_starts_at_route = chain_text.contains("Route::");
        let k = match func_name.to_ascii_lowercase().as_str() {
            "route" | "signedroute" | "temporarysignedroute" | "redirecttoroute" | "routeis" => {
                Some(LaravelStringKind::Route)
            }
            // `$this->call('cmd')` / `$this->callSilently('cmd')` inside a
            // console command run another Artisan command.  Restricted to a
            // `$this` receiver because `->call()` is a common method name.
            "call" | "callsilently" if receiver_is_this => Some(LaravelStringKind::Command),
            // `$this->authorize('update', $post)` in a controller.
            "authorize" if receiver_is_this || chain_starts_at_gate => {
                Some(LaravelStringKind::GateAbility)
            }
            // `$user->can('update', $post)`.
            "can" | "cannot" | "canany"
                if receiver_is_user_like || chain_starts_at_route || chain_starts_at_gate =>
            {
                Some(LaravelStringKind::GateAbility)
            }
            "allows" | "denies" | "check" | "any" | "none" | "inspect" | "has"
                if chain_starts_at_gate =>
            {
                Some(LaravelStringKind::GateAbility)
            }
            _ => None,
        };
        (k, None)
    } else {
        if argument.named_argument.is_some() || argument.shape != StringArgumentShape::Scalar {
            return None;
        }
        match func_name.to_ascii_lowercase().as_str() {
            "route" | "to_route" => (Some(LaravelStringKind::Route), None),
            "config" => (Some(LaravelStringKind::Config), None),
            "view" | "blade_view_directive" | "blade_each_directive" => {
                (Some(LaravelStringKind::View), None)
            }
            "__" | "trans" | "trans_choice" => (Some(LaravelStringKind::Trans), None),
            "env" => (Some(LaravelStringKind::Env), None),
            // The Blade preprocessor lowers `@can`/`@cannot`/`@canany` to
            // this call, so completion inside the directive works too.
            "blade_can_directive" => (Some(LaravelStringKind::GateAbility), None),
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
    /// The configured Blade view root directories.
    ///
    /// Reads the `paths` array from `config/view.php` (falling back to
    /// the conventional `resources/views`) so that projects with custom
    /// view directories resolve `view()` names correctly. Only existing
    /// directories are returned. Read from disk, so unsaved edits to
    /// `config/view.php` are not reflected until saved.
    pub(crate) fn laravel_view_roots(&self) -> Vec<std::path::PathBuf> {
        match self.workspace.workspace_root.read().clone() {
            Some(root) => crate::blade::discover_view_paths(&root),
            None => Vec::new(),
        }
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

    /// Enumerate every translation key alongside whether it names a
    /// translation group (a nested array) rather than a scalar string
    /// entry, merging the flag across every locale and file that
    /// declares the key.
    ///
    /// A key that is a group in *any* locale is recorded as a group even
    /// if another locale happens to declare it as a scalar — the return
    /// type narrowing this feeds is only safe when every locale agrees
    /// the entry is scalar.
    fn enumerate_all_trans_key_shapes(&self) -> HashMap<String, bool> {
        let snapshot = self.user_file_symbol_maps();
        let mut shapes = HashMap::new();

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
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.trans_keys,
            |cache| cache.trans_keys.clone(),
            |cache, keys| cache.trans_keys = Some(keys),
            || self.enumerate_all_trans_keys(),
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
    let root = match backend.workspace.workspace_root.read().clone() {
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

/// Record a key's group/scalar shape, OR-ing into any flag already
/// recorded for the same key from another locale or file.
fn mark_trans_shape(shapes: &mut HashMap<String, bool>, key: String, is_group: bool) {
    let existing = shapes.entry(key).or_insert(false);
    *existing = *existing || is_group;
}

/// The shape counterpart of [`collect_json_trans_keys`]: every JSON
/// translation key is a scalar phrase, never a group.
fn collect_json_trans_key_shapes(backend: &crate::Backend, out: &mut HashMap<String, bool>) {
    let root = match backend.workspace.workspace_root.read().clone() {
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
                    mark_trans_shape(out, k.clone(), false);
                }
            }
        }
    }
}

/// The shape counterpart of [`collect_namespaced_trans_keys`].
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

// ─── Completion ─────────────────────────────────────────────────────────────

/// The icon an editor shows beside a completed string key: whatever the key
/// names is what it should look like.
fn string_key_item_kind(kind: &LaravelStringKind) -> CompletionItemKind {
    match kind {
        LaravelStringKind::Config => CompletionItemKind::PROPERTY,
        LaravelStringKind::View => CompletionItemKind::FILE,
        LaravelStringKind::Trans => CompletionItemKind::TEXT,
        LaravelStringKind::MorphAlias => CompletionItemKind::ENUM_MEMBER,
        LaravelStringKind::GateAbility => CompletionItemKind::METHOD,
        LaravelStringKind::Env => CompletionItemKind::CONSTANT,
        LaravelStringKind::Route
        | LaravelStringKind::Command
        | LaravelStringKind::Section
        | LaravelStringKind::Stack
        | LaravelStringKind::ContainerBinding => CompletionItemKind::VALUE,
    }
}

impl Backend {
    /// Every name a string key of `kind` could be, unfiltered.
    ///
    /// Three kinds have no list to offer. A Blade section or stack name is
    /// completed from the raw template instead
    /// (`crate::completion::handler::blade_block_name`): what a name may be
    /// depends on the layouts above the file, and the edit has to land in
    /// Blade coordinates rather than in the virtual PHP this detection reads.
    /// A container binding key is written where a class name is equally
    /// valid, which ordinary class completion already offers, and the set of
    /// keys is open besides — a list of them would read as the whole answer
    /// when it is not.
    fn string_key_candidates(&self, kind: &LaravelStringKind) -> Vec<String> {
        match kind {
            LaravelStringKind::Route => self.cached_route_names(),
            LaravelStringKind::Config => self.cached_config_keys(),
            LaravelStringKind::View => self.cached_view_names(),
            LaravelStringKind::Trans => self.cached_trans_keys(),
            LaravelStringKind::Command => self.laravel_commands.read().all_names(),
            LaravelStringKind::MorphAlias => {
                let mut aliases = self.laravel_morph_map.read().all_aliases();
                aliases.sort();
                aliases
            }
            LaravelStringKind::GateAbility => self.cached_gate_abilities(),
            LaravelStringKind::Env => crate::virtual_members::laravel::enumerate_env_keys(self),
            LaravelStringKind::Section
            | LaravelStringKind::Stack
            | LaravelStringKind::ContainerBinding => Vec::new(),
        }
    }

    /// Try Laravel string key completion.
    ///
    /// Detects the cursor inside a supported string argument of `route()`,
    /// `config()`, `Storage::forgetDisk()`, etc. and offers matching names.
    pub(crate) fn try_laravel_string_key_completion(
        &self,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let ctx = detect_laravel_string_key_context(content, position)?;

        let mut candidates = self.string_key_candidates(&ctx.kind);

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
        let start_pos = crate::text_position::offset_to_position(content, ctx.content_start_offset);
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
                let kind = string_key_item_kind(&ctx.kind);
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

    fn detect_at_end(content: &str, value: &str) -> Option<LaravelStringKeyContext> {
        let cursor = content.rfind(value)? + value.len();
        detect_laravel_string_key_context(
            content,
            crate::text_position::offset_to_position(content, cursor),
        )
    }

    fn storage_source(expression: &str) -> String {
        format!("<?php\nuse Illuminate\\Support\\Facades\\Storage;\n{expression};\n")
    }

    fn completion_response_items(response: CompletionResponse) -> Vec<CompletionItem> {
        match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        }
    }

    /// Three kinds are recorded as spans but completed from somewhere else,
    /// or not at all, so they must offer nothing here rather than an empty
    /// list dressed up as the answer.
    #[test]
    fn the_kinds_completed_elsewhere_offer_no_candidates() {
        let backend = crate::test_fixtures::make_backend();
        for kind in [
            LaravelStringKind::Section,
            LaravelStringKind::Stack,
            LaravelStringKind::ContainerBinding,
        ] {
            assert!(
                backend.string_key_candidates(&kind).is_empty(),
                "{kind:?} should offer no candidates"
            );
        }
    }

    /// Whatever a key names decides the icon beside it.
    #[test]
    fn a_string_key_is_iconed_by_what_it_names() {
        use tower_lsp::lsp_types::CompletionItemKind;
        for (kind, expected) in [
            (LaravelStringKind::Config, CompletionItemKind::PROPERTY),
            (LaravelStringKind::View, CompletionItemKind::FILE),
            (LaravelStringKind::Trans, CompletionItemKind::TEXT),
            (
                LaravelStringKind::MorphAlias,
                CompletionItemKind::ENUM_MEMBER,
            ),
            (LaravelStringKind::GateAbility, CompletionItemKind::METHOD),
            (LaravelStringKind::Route, CompletionItemKind::VALUE),
            (LaravelStringKind::Command, CompletionItemKind::VALUE),
            (LaravelStringKind::Section, CompletionItemKind::VALUE),
            (LaravelStringKind::Stack, CompletionItemKind::VALUE),
            (
                LaravelStringKind::ContainerBinding,
                CompletionItemKind::VALUE,
            ),
        ] {
            assert_eq!(string_key_item_kind(&kind), expected, "for {kind:?}");
        }
    }

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

    /// Every call that names a route completes route names, whichever of
    /// the URL-building helpers it hangs off.
    #[test]
    fn detects_the_route_helpers_beyond_the_route_function() {
        for call in [
            "URL::signedRoute('orders.",
            "Redirect::temporarySignedRoute('orders.",
            "Response::redirectToRoute('orders.",
            "Route::is('orders.",
            "redirect()->route('orders.",
            "$request->routeIs('orders.",
        ] {
            let content = format!("<?php\n{call}");
            let col = content.lines().nth(1).unwrap().len() as u32;
            let ctx = detect_laravel_string_key_context(&content, Position::new(1, col))
                .unwrap_or_else(|| panic!("should detect `{call}` as a route context"));
            assert!(matches!(ctx.kind, LaravelStringKind::Route), "for `{call}`");
            assert_eq!(ctx.prefix, "orders.", "for `{call}`");
        }
    }

    /// A `getMany()` key is the first entry of an array literal rather than
    /// the first argument, and `Env::get()` names an environment variable.
    #[test]
    fn detects_the_list_and_environment_call_shapes() {
        let content = "<?php\nConfig::getMany(['app.";
        let col = content.lines().nth(1).unwrap().len() as u32;
        let ctx = detect_laravel_string_key_context(content, Position::new(1, col))
            .expect("should detect getMany() as a config context");
        assert!(matches!(ctx.kind, LaravelStringKind::Config));
        assert_eq!(ctx.prefix, "app.");

        for call in ["Env::get('APP_", "env('APP_"] {
            let content = format!("<?php\n{call}");
            let col = content.lines().nth(1).unwrap().len() as u32;
            let ctx = detect_laravel_string_key_context(&content, Position::new(1, col))
                .unwrap_or_else(|| panic!("should detect `{call}` as an environment context"));
            assert!(matches!(ctx.kind, LaravelStringKind::Env), "for `{call}`");
            assert_eq!(ctx.prefix, "APP_", "for `{call}`");
        }
    }

    /// `#[RedirectToRoute]` names a route rather than a config key, but only
    /// where the file imports Laravel's attribute.
    #[test]
    fn detects_the_redirect_to_route_attribute() {
        let content = "<?php\nuse Illuminate\\Foundation\\Http\\Attributes\\RedirectToRoute;\n\
                       #[RedirectToRoute('lo";
        let col = content.lines().nth(2).unwrap().len() as u32;
        let ctx = detect_laravel_string_key_context(content, Position::new(2, col))
            .expect("should detect the attribute as a route context");
        assert!(matches!(ctx.kind, LaravelStringKind::Route));
        assert_eq!(ctx.prefix, "lo");

        let unimported = "<?php\n#[RedirectToRoute('lo";
        let col = unimported.lines().nth(1).unwrap().len() as u32;
        assert!(
            detect_laravel_string_key_context(unimported, Position::new(1, col)).is_none(),
            "an attribute of the same name from elsewhere names no route"
        );
    }

    /// The preprocessor compiles Blade's render directives into marker
    /// calls, so completion inside `@include('` and `@each('` reaches the
    /// view index through those names.
    #[test]
    fn detects_the_blade_render_directive_markers() {
        for marker in ["blade_view_directive", "blade_each_directive"] {
            let content = format!("<?php\n{marker} ('partials.');\n");
            let line_text = content.lines().nth(1).unwrap();
            let col = line_text.find("partials.").unwrap() as u32 + 9;
            let ctx = detect_laravel_string_key_context(&content, Position::new(1, col));
            let ctx = ctx.unwrap_or_else(|| panic!("should detect {marker} context"));
            assert!(matches!(ctx.kind, LaravelStringKind::View));
            assert_eq!(ctx.prefix, "partials.");
        }
    }

    #[test]
    fn detects_artisan_call_command() {
        let content = "<?php\nArtisan::call('app:');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("app:").unwrap() as u32 + 4;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect Artisan::call() context");
        assert!(matches!(ctx.kind, LaravelStringKind::Command));
    }

    #[test]
    fn detects_this_call_command() {
        let content = "<?php\n$this->call('app:');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("app:").unwrap() as u32 + 4;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect $this->call() context");
        assert!(matches!(ctx.kind, LaravelStringKind::Command));
    }

    #[test]
    fn rejects_non_this_call() {
        // `->call()` on an arbitrary object is not a command reference.
        let content = "<?php\n$service->call('doSomething');\n";
        let line = 1;
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("doSomething").unwrap() as u32 + 2;
        let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
        assert!(
            ctx.is_none(),
            "->call() on a non-$this receiver should not match"
        );
    }

    #[test]
    fn detects_gate_facade_ability() {
        let content = "<?php\nGate::allows('upd');\n";
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("upd").unwrap() as u32 + 3;
        let ctx = detect_laravel_string_key_context(content, Position::new(1, col))
            .expect("should detect Gate::allows() context");
        assert!(matches!(ctx.kind, LaravelStringKind::GateAbility));
        assert_eq!(ctx.prefix, "upd");
    }

    /// Every entry point that names an ability completes from the same set.
    #[test]
    fn detects_every_ability_call_shape() {
        for call in [
            // The facade's own API, including the registration itself.
            "Gate::allows('upd'",
            "Gate::denies('upd'",
            "Gate::check('upd'",
            "Gate::any('upd'",
            "Gate::none('upd'",
            "Gate::authorize('upd'",
            "Gate::inspect('upd'",
            "Gate::has('upd'",
            "Gate::define('upd'",
            // A chain rooted at the facade.
            "Gate::forUser($user)->allows('upd'",
            "Gate::forUser($user)->authorize('upd'",
            "Gate::forUser($user)->has('upd'",
            // A controller's own helper.
            "$this->authorize('upd'",
            // The user the check is about.
            "$user->can('upd'",
            "$user->cannot('upd'",
            "$user->canAny('upd'",
            // A route registration.
            "Route::get('/p', $a)->can('upd'",
            // The Blade `@can` directive, after preprocessing.
            "blade_can_directive('upd'",
        ] {
            let content = format!("<?php\n{call});\n");
            let line_text = content.lines().nth(1).unwrap();
            let col = line_text.rfind("upd").unwrap() as u32 + 3;
            let ctx = detect_laravel_string_key_context(&content, Position::new(1, col))
                .unwrap_or_else(|| panic!("`{call}` should offer abilities"));
            assert!(
                matches!(ctx.kind, LaravelStringKind::GateAbility),
                "`{call}` should offer abilities, got {:?}",
                ctx.kind
            );
            assert_eq!(ctx.prefix, "upd", "for `{call}`");
        }
    }

    #[test]
    fn detects_user_can_ability() {
        let content = "<?php\n$user->can('upd', $post);\n";
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("upd").unwrap() as u32 + 3;
        let ctx = detect_laravel_string_key_context(content, Position::new(1, col))
            .expect("should detect $user->can() context");
        assert!(matches!(ctx.kind, LaravelStringKind::GateAbility));
    }

    #[test]
    fn rejects_can_on_an_unrelated_receiver() {
        let content = "<?php\n$rules->can('read');\n";
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("read").unwrap() as u32 + 4;
        assert!(
            detect_laravel_string_key_context(content, Position::new(1, col)).is_none(),
            "->can() on a receiver that is not a user must not match"
        );
    }

    /// A `Gate::` call earlier in the file must not turn an unrelated
    /// `->has()` several statements later into an ability check.
    #[test]
    fn a_gate_call_in_an_earlier_statement_does_not_leak() {
        let content = "<?php\nGate::allows('update');\n$bag->has('key');\n";
        let line_text = content.lines().nth(2).unwrap();
        let col = line_text.find("key").unwrap() as u32 + 3;
        assert!(
            detect_laravel_string_key_context(content, Position::new(2, col)).is_none(),
            "an earlier Gate:: statement must not reach a later chain"
        );
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
    fn detects_static_config_mutators_and_translation_queries() {
        for method in ["prepend", "push"] {
            let content = format!("<?php\nConfig::{method}('app.providers');\n");
            let ctx = detect_at_end(&content, "app.providers")
                .unwrap_or_else(|| panic!("should detect Config::{method}() context"));
            assert_eq!(ctx.kind, LaravelStringKind::Config);
        }

        for method in ["get", "has", "hasForLocale", "choice"] {
            let content = format!("<?php\nLang::{method}('messages.saved');\n");
            let ctx = detect_at_end(&content, "messages.saved")
                .unwrap_or_else(|| panic!("should detect Lang::{method}() context"));
            assert_eq!(ctx.kind, LaravelStringKind::Trans);
        }
    }

    #[test]
    fn storage_argument_scanning_preserves_existing_static_contexts() {
        for (expression, expected_kind, expected_prefix) in [
            (
                "DB::connection('primary')",
                LaravelStringKind::Config,
                Some("database.connections."),
            ),
            (
                "Model::getActualClassNameForMorph('post')",
                LaravelStringKind::MorphAlias,
                None,
            ),
        ] {
            let content = format!("<?php\n{expression};\n");
            let value = if expected_prefix.is_some() {
                "primary"
            } else {
                "post"
            };
            let ctx = detect_at_end(&content, value)
                .unwrap_or_else(|| panic!("should preserve `{expression}` completion"));
            assert_eq!(ctx.kind, expected_kind);
            assert_eq!(ctx.config_sub_prefix, expected_prefix);
        }
    }

    #[test]
    fn detects_storage_disk_scalar_methods_and_named_arguments() {
        for expression in [
            "Storage::disk('arch')",
            "Storage::disk(name: 'arch')",
            "Storage::fake('arch')",
            "Storage::fake(disk: 'arch')",
            "Storage::persistentFake('arch')",
            "Storage::persistentFake(disk: 'arch')",
            "Storage::forgetDisk('arch')",
            "Storage::forgetDisk(disk: 'arch')",
        ] {
            let content = storage_source(expression);
            let ctx = detect_at_end(&content, "arch")
                .unwrap_or_else(|| panic!("should detect `{expression}` as a disk context"));
            assert!(matches!(ctx.kind, LaravelStringKind::Config));
            assert_eq!(ctx.prefix, "arch", "prefix for `{expression}`");
            assert_eq!(
                ctx.config_sub_prefix,
                Some("filesystems.disks."),
                "config subtree for `{expression}`"
            );
        }

        for expression in [
            "\\Illuminate\\Support\\Facades\\Storage::disk(name: 'arch')",
            "#[\\Illuminate\\Container\\Attributes\\Storage(disk: 'arch')] class Consumer {}",
        ] {
            let content = format!("<?php\n{expression};\n");
            let ctx = detect_at_end(&content, "arch")
                .unwrap_or_else(|| panic!("should detect `{expression}` as a disk context"));
            assert!(matches!(ctx.kind, LaravelStringKind::Config));
            assert_eq!(ctx.config_sub_prefix, Some("filesystems.disks."));
        }
    }

    #[test]
    fn storage_facade_resolution_accepts_imports_and_rejects_homonyms() {
        for content in [
            "<?php\nStorage::disk('arch');\n",
            "<?php\nuse Illuminate\\Support\\Facades\\Storage as Disks;\nDisks::disk('arch');\n",
            "<?php\nuse Illuminate\\Support\\Facades\\{Storage as Disks, Cache};\nDisks::disk('arch');\n",
            "<?php\nuse Vendor\\Package\\{Cache};\nuse Illuminate\\Support\\Facades\\Storage as Disks;\nDisks::disk('arch');\n",
        ] {
            assert!(
                detect_at_end(content, "arch").is_some(),
                "source: {content}"
            );
        }

        for content in [
            "<?php\nVendor\\Storage::disk('arch');\n",
            "<?php\nuse Vendor\\Storage;\nStorage::disk('arch');\n",
            "<?php\nnamespace App;\nuse Vendor\\{Storage as Disks};\nDisks::disk('arch');\n",
            "<?php\nnamespace App;\nuse Illuminate\\Support\\Facades\\{Storage;\nStorage::disk('arch');\n",
            "<?php\nnamespace App;\nclass Storage {}\nStorage::disk('arch');\n",
        ] {
            assert!(
                detect_at_end(content, "arch").is_none(),
                "source: {content}"
            );
        }

        const FACADE: &str = "Illuminate\\Support\\Facades\\Storage";
        assert_eq!(
            imported_item_is_storage("Storage nope", None, FACADE, "Storage"),
            None
        );
        assert_eq!(
            imported_item_is_storage("Storage as Disks trailing", None, FACADE, "Disks"),
            None
        );
    }

    #[test]
    fn detects_every_forget_disk_array_value_and_spelling() {
        for expression in [
            "Storage::forgetDisk(['archive', 'backup'])",
            "Storage::forgetDisk(array('archive', 'backup'))",
            "Storage::forgetDisk(disk: ['archive', 'backup'])",
            "Storage::forgetDisk(disk: array('archive', 'backup'))",
        ] {
            let content = storage_source(expression);
            for value in ["archive", "backup"] {
                let cursor = content.find(value).unwrap() + value.len();
                let ctx = detect_laravel_string_key_context(
                    &content,
                    crate::text_position::offset_to_position(&content, cursor),
                )
                .unwrap_or_else(|| panic!("should detect `{value}` in `{expression}`"));
                assert!(matches!(ctx.kind, LaravelStringKind::Config));
                assert_eq!(ctx.prefix, value);
                assert_eq!(ctx.config_sub_prefix, Some("filesystems.disks."));
            }
        }
    }

    #[test]
    fn forget_disk_array_completion_filters_and_edits_the_current_value() {
        let backend = crate::Backend::new_test();
        backend.laravel_string_key_cache.write().config_keys = Some(vec![
            "cache.stores.archive".to_string(),
            "filesystems.disks.archive".to_string(),
            "filesystems.disks.archive.driver".to_string(),
            "filesystems.disks.backup".to_string(),
        ]);

        let content = "<?php\nuse Illuminate\\Support\\Facades\\Storage;\nStorage::forgetDisk(['backup', 'ar']);\n";
        let cursor = content.rfind("ar").unwrap() + 2;
        let position = crate::text_position::offset_to_position(content, cursor);
        let response = backend
            .try_laravel_string_key_completion(content, position)
            .expect("the array value should offer disk names");
        let items = completion_response_items(response);
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec!["archive"]
        );
        assert_eq!(
            items[0].text_edit,
            Some(CompletionTextEdit::Edit(TextEdit {
                range: Range::new(
                    crate::text_position::offset_to_position(content, cursor - 2),
                    position,
                ),
                new_text: "archive".to_string(),
            }))
        );
    }

    #[test]
    fn completion_response_item_helper_accepts_list_responses() {
        let items = completion_response_items(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items: vec![CompletionItem::default()],
        }));
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn forget_disk_arrays_complete_values_but_not_associative_keys() {
        let content = "<?php\nuse Illuminate\\Support\\Facades\\Storage;\nStorage::forgetDisk(['alias' => 'archive']);\n";
        let key_cursor = content.find("alias").unwrap() + "alias".len();
        assert!(
            detect_laravel_string_key_context(
                content,
                crate::text_position::offset_to_position(content, key_cursor),
            )
            .is_none(),
            "an associative key is not a disk name"
        );

        let value = detect_at_end(content, "archive").expect("the array value is a disk name");
        assert!(matches!(value.kind, LaravelStringKind::Config));
        assert_eq!(value.prefix, "archive");
    }

    #[test]
    fn rejects_invalid_storage_argument_names_and_shapes() {
        for expression in [
            "Storage::disk(['archive'])",
            "Storage::fake(['archive'])",
            "Storage::persistentFake(['archive'])",
            "Storage::forgetDisk([['archive']])",
            "Storage::disk(disk: 'archive')",
            "Storage::fake(name: 'archive')",
            "Storage::persistentFake(name: 'archive')",
            "Storage::forgetDisk(name: 'archive')",
            "#[\\Illuminate\\Container\\Attributes\\Storage(name: 'archive')] class C {}",
            "#[\\Illuminate\\Container\\Attributes\\Storage(disk: ['archive'])] class C {}",
            "Storage::extend('archive', fn () => null)",
            "Config::get(['archive'])",
            "route(name: 'archive')",
        ] {
            let content = storage_source(expression);
            assert!(
                detect_at_end(&content, "archive").is_none(),
                "`{expression}` must not offer disk completion"
            );
        }
    }

    #[test]
    fn storage_argument_scanners_handle_nested_and_malformed_php() {
        let before_named = r#"Storage::fake(config: ['message' => 'it\'s', 'factory' => wrap(fn () => new class {})], disk:"#;
        assert_eq!(
            callable_before_scalar_argument(before_named),
            Some(("Storage::fake", Some("disk")))
        );

        let nested = r#"<?php
use Illuminate\Support\Facades\Storage;
Storage::forgetDisk([['nested'], wrap(fn () => new class {}), 'it\'s', 'archive']);"#;
        let ctx = detect_at_end(nested, "archive")
            .expect("balanced nested expressions must not hide the outer array");
        assert_eq!(ctx.config_sub_prefix, Some("filesystems.disks."));

        assert!(callable_before_scalar_argument("Storage::disk(:").is_none());
        assert!(callable_before_scalar_argument("Storage::disk(name: value").is_none());
        assert!(callable_before_scalar_argument("orphan name:").is_none());
        assert!(enclosing_call_open_paren("completed(); orphan").is_none());
        assert!(enclosing_call_open_paren("orphan").is_none());
        assert!(callable_before_array_argument("factory('archive").is_none());
        assert!(callable_before_array_argument("{ 'archive").is_none());
        assert!(callable_before_array_argument("broken; 'archive").is_none());
        assert!(callable_before_array_argument("orphan").is_none());

        let escaped_key = r#"'key\'part' => 'archive'"#;
        assert!(string_literal_is_array_key(
            escaped_key,
            "'key".len(),
            b'\''
        ));
        assert!(!string_literal_is_array_key("'key\n", "'key".len(), b'\''));
        assert!(!string_literal_is_array_key("'key", "'key".len(), b'\''));
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

    /// Concurrent first callers must share one build, not run one each:
    /// every enumeration behind these accessors walks the workspace from
    /// disk, and the diagnostic pass hits them from all N workers at once.
    #[test]
    fn concurrent_first_callers_build_the_enumeration_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let backend = crate::Backend::new_test();
        let builds = AtomicUsize::new(0);
        let build_lock = parking_lot::Mutex::new(());

        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    let backend = &backend;
                    let builds = &builds;
                    let build_lock = &build_lock;
                    scope.spawn(move || {
                        backend.cached_laravel_enumeration(
                            build_lock,
                            |cache| cache.view_names.clone(),
                            |cache, names| cache.view_names = Some(names),
                            || {
                                builds.fetch_add(1, Ordering::SeqCst);
                                // Long enough that an unguarded
                                // check-then-fill has every thread miss.
                                std::thread::sleep(std::time::Duration::from_millis(50));
                                vec!["home".to_string()]
                            },
                        )
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        assert_eq!(
            builds.load(Ordering::SeqCst),
            1,
            "the enumeration must be built once and shared, not once per caller"
        );
        for names in &results {
            assert_eq!(names, &vec!["home".to_string()]);
        }
    }
}
