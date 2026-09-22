//! What a Laravel string key call site looks like, read from the raw
//! buffer.
//!
//! Detection is textual rather than AST-based: it runs on a buffer that is
//! mid-edit, where the call being typed usually does not parse yet.  The
//! symbol map decides the same question for a *complete* file, and the two
//! are kept in step by hand.

use tower_lsp::lsp_types::Position;

use crate::symbol_map::LaravelStringKind;
use crate::text_position::position_to_offset;
use crate::virtual_members::laravel::{is_storage_facade_name, storage_facade_local_names};

pub(super) struct LaravelStringKeyContext {
    pub(super) kind: LaravelStringKind,
    pub(super) prefix: String,
    /// Byte offset of the string content start (right after the opening quote).
    pub(super) content_start_offset: usize,
    /// When set, the key is a sub-key under this config path prefix.
    /// For example, `#[Database('mysql')]` sets this to `"database.connections."`
    /// so completion filters to `database.connections.*` keys and strips the
    /// prefix, showing just `mysql`, `sqlite`, etc.
    pub(super) config_sub_prefix: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StringArgumentShape {
    Scalar,
    ArrayValue,
}

pub(super) struct StringArgumentContext<'a> {
    pub(super) callable: &'a str,
    pub(super) named_argument: Option<&'a str>,
    pub(super) shape: StringArgumentShape,
}

#[inline]
pub(super) fn is_unescaped(bytes: &[u8], index: usize) -> bool {
    let mut before = index;
    while before > 0 && bytes[before - 1] == b'\\' {
        before -= 1;
    }
    (index - before).is_multiple_of(2)
}

/// Find the unmatched call parenthesis enclosing a named argument.
pub(super) fn enclosing_call_open_paren(content: &str) -> Option<usize> {
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
pub(super) fn callable_before_scalar_argument(before_value: &str) -> Option<(&str, Option<&str>)> {
    let before_value = before_value.trim_end();
    if let Some(callable) = before_value.strip_suffix('(') {
        return Some((callable.trim_end(), None));
    }

    let colon = before_value.rfind(':')?;
    if !before_value[colon + 1..].trim().is_empty() {
        return None;
    }
    let before_label = before_value[..colon].trim_end();
    // Scanned by byte, the way PHP's lexer reads a label: every byte from
    // 0x80 up is a label byte, so a non-ASCII name (`prénom:`) is read
    // whole and the start always lands on a character boundary.
    let label_start = before_label
        .bytes()
        .rposition(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80))
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
pub(super) fn callable_before_array_argument(before_quote: &str) -> Option<(&str, Option<&str>)> {
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
pub(super) fn string_literal_is_array_key(content: &str, cursor: usize, quote: u8) -> bool {
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

pub(super) fn string_argument_context<'a>(
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

// ─── Detection ──────────────────────────────────────────────────────────────

/// Detect if the cursor is inside a supported string argument of a Laravel
/// helper or facade call. Returns the key kind and the prefix typed so far.
pub(super) fn detect_laravel_string_key_context(
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
        // `#[Storage]` turns any argument into a `filesystems.disks.*` key,
        // so an application's own same-named attribute would invent one.
        // The short spelling therefore has to be imported by that exact
        // name, matching what the symbol map records.
        let storage_attr_matches = || {
            if is_fqn {
                attr_class == format!("{ATTR_NS}Storage")
            } else {
                short == "Storage"
                    && crate::text_scan::imports_class_as(
                        content,
                        &format!("{ATTR_NS}Storage"),
                        "Storage",
                    )
            }
        };

        // `#[Storage(disk: '…')]` is the one container attribute whose
        // argument is recognised by name.
        if let Some(name) = argument.named_argument
            && !(storage_attr_matches() && name.eq_ignore_ascii_case("disk"))
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
        } else if storage_attr_matches() {
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

        let fn_lower = func_name.to_ascii_lowercase();
        let short_lower = short.to_ascii_lowercase();

        // The `Storage` facade's disk-name arguments: the parameter the disk
        // goes in, and whether that parameter also accepts a list.  The
        // facade is only resolved through the file's imports once a method
        // name has matched — `disk()`, `fake()` and `forgetDisk()` are common
        // names on unrelated facades, and resolving scans the whole buffer.
        let storage_argument = match fn_lower.as_str() {
            "disk" => Some(("name", false)),
            "fake" | "persistentfake" => Some(("disk", false)),
            "forgetdisk" => Some(("disk", true)),
            _ => None,
        }
        .filter(|_| is_storage_facade_name(class_name, &storage_facade_local_names(content)));
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
        let fn_lower = func_name.to_ascii_lowercase();
        // The Blade preprocessor lowers `@includeFirst`/`@componentFirst`/
        // `@extendsFirst` and `@canany` to markers that name their candidates
        // inside an array literal rather than as a plain first argument.
        let accepts_array = matches!(
            fn_lower.as_str(),
            "blade_view_directive" | "blade_can_directive"
        );
        if argument.named_argument.is_some()
            || (argument.shape != StringArgumentShape::Scalar
                && (!accepts_array || !before_quote.trim_end().ends_with('[')))
        {
            return None;
        }
        match fn_lower.as_str() {
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
