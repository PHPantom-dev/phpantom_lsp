/// Array shape key completion.
///
/// This module detects when the cursor is inside an array access expression
/// with a string key (e.g. `$config['`) and offers completion items for
/// the known keys of the array shape type annotation.
///
/// It also provides helpers for resolving the raw type annotation of a
/// variable so that array shape entries can be extracted.
///
/// # Supported annotation sources
///
/// - `/** @var array{name: string, age: int} $var */` — inline `@var`
/// - `@param array{name: string, age: int} $param` — method/function parameter
/// - `@return array{name: string}` — return type of a function/method call
///   assigned to the variable
/// - Property type annotations (`@var` on class properties)
/// - `$_SERVER` superglobal — hardcoded key completions for all 40 well-known keys
///
/// # Auto-close handling
///
/// Completion items use `text_edit` with a range that covers any trailing
/// auto-inserted characters (closing quote + `]`) placed by the IDE.
/// This prevents duplicates like `$config['host']]` or `$config['host']']`.
///
/// # Nested array shapes
///
/// Chained array accesses like `$response['meta']['` are supported.
/// The detector collects prefix keys (`["meta"]`) and the resolver walks
/// through each level of the shape to offer keys from the inner type.
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::resolver::Loaders;
use crate::docblock;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FileContext, ResolvedType};
use crate::util::{find_class_at_offset, position_to_offset};

/// Well-known keys for the `$_SERVER` superglobal.
///
/// Each entry is `(key, detail)` where `detail` is a short description
/// shown next to the completion item.
const SERVER_KEYS: &[(&str, &str)] = &[
    ("PHP_SELF", "string — Current script filename"),
    ("argv", "array — Arguments passed to the script"),
    ("argc", "int — Number of arguments passed to the script"),
    ("GATEWAY_INTERFACE", "string — CGI specification revision"),
    ("SERVER_ADDR", "string — Server IP address"),
    ("SERVER_NAME", "string — Server hostname"),
    ("SERVER_SOFTWARE", "string — Server identification string"),
    (
        "SERVER_PROTOCOL",
        "string — Name and revision of the protocol",
    ),
    ("REQUEST_METHOD", "string — Request method (GET, POST, …)"),
    ("REQUEST_TIME", "int — Timestamp of the request start"),
    ("REQUEST_TIME_FLOAT", "float — Timestamp with microseconds"),
    ("QUERY_STRING", "string — The query string"),
    ("DOCUMENT_ROOT", "string — Document root directory"),
    ("HTTP_ACCEPT", "string — Accept header contents"),
    ("HTTP_ACCEPT_CHARSET", "string — Accept-Charset header"),
    ("HTTP_ACCEPT_ENCODING", "string — Accept-Encoding header"),
    ("HTTP_ACCEPT_LANGUAGE", "string — Accept-Language header"),
    ("HTTP_CONNECTION", "string — Connection header"),
    ("HTTP_HOST", "string — Host header"),
    ("HTTP_REFERER", "string — Referring page URL"),
    ("HTTP_USER_AGENT", "string — User agent string"),
    ("HTTPS", "string — Set to 'on' if HTTPS is used"),
    ("REMOTE_ADDR", "string — Client IP address"),
    ("REMOTE_HOST", "string — Client hostname"),
    ("REMOTE_PORT", "string — Client port"),
    ("REMOTE_USER", "string — Authenticated user"),
    (
        "REDIRECT_REMOTE_USER",
        "string — Authenticated user (redirect)",
    ),
    ("SCRIPT_FILENAME", "string — Absolute path of the script"),
    ("SERVER_ADMIN", "string — SERVER_ADMIN directive value"),
    ("SERVER_PORT", "string — Server port"),
    ("SERVER_SIGNATURE", "string — Server signature string"),
    ("PATH_TRANSLATED", "string — Filesystem path of the script"),
    ("SCRIPT_NAME", "string — Current script path"),
    ("REQUEST_URI", "string — URI used to access the page"),
    ("PHP_AUTH_DIGEST", "string — Digest HTTP auth header"),
    ("PHP_AUTH_USER", "string — HTTP auth username"),
    ("PHP_AUTH_PW", "string — HTTP auth password"),
    ("AUTH_TYPE", "string — Authentication type"),
    ("PATH_INFO", "string — Client-provided path info"),
    ("ORIG_PATH_INFO", "string — Original PATH_INFO"),
];

/// The result of detecting an array key completion context.
///
/// Returned by [`detect_array_key_context`] when the cursor is positioned
/// inside an array access with a string key (or right after `[`).
#[derive(Debug, Clone)]
pub(crate) struct ArrayKeyContext {
    /// The variable name including the `$` prefix (e.g. `"$config"`).
    pub var_name: String,
    /// The partial key the user has typed so far (may be empty).
    /// Does **not** include the opening quote character.
    pub partial_key: String,
    /// The quote character used (`'` or `"`), or `None` if the user
    /// typed `[` without a quote yet.
    pub quote_char: Option<char>,
    /// The column (0-based) where the key text begins on the cursor line.
    /// This is right after the opening quote (if any) or right after `[`.
    pub key_start_col: u32,
    /// Keys from preceding chained array accesses, innermost first.
    ///
    /// For `$response['meta']['page'][`, this would be `["meta", "page"]`
    /// so the resolver can walk through nested array shapes.
    pub prefix_keys: Vec<String>,
}

/// Detect whether the cursor is in an array key completion context.
///
/// Recognises patterns like:
///   - `$var['`                    — empty partial, single-quote
///   - `$var['na`                  — partial "na", single-quote
///   - `$var["`                    — empty partial, double-quote
///   - `$var["na`                  — partial "na", double-quote
///   - `$var[`                     — no quote yet
///   - `$var['key1']['key2'][`     — chained access (nested shapes)
///   - `$var['key1']['key2']['`    — chained access with quote
///
/// Returns `None` if the cursor is not in such a context.
pub(crate) fn detect_array_key_context(
    content: &str,
    position: Position,
) -> Option<ArrayKeyContext> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;
    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let chars: Vec<char> = line.chars().collect();
    let col = (position.character as usize).min(chars.len());

    if col == 0 {
        return None;
    }

    // Walk backward from the cursor to find the pattern.
    let mut i = col;

    // 1. Collect partial key text (identifier characters the user has typed).
    let partial_end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }
    let partial_start = i;

    // 2. Check for a quote character.
    let quote_char = if i > 0 && (chars[i - 1] == '\'' || chars[i - 1] == '"') {
        let q = chars[i - 1];
        i -= 1;
        Some(q)
    } else {
        None
    };

    // 3. Must have `[` immediately before the quote (or the partial if no quote).
    if i == 0 || chars[i - 1] != '[' {
        return None;
    }
    i -= 1; // skip `[`

    let key_start_col = partial_start as u32;

    // 4. Try to collect chained `['key']` access segments before the
    //    current `[`.  Walk backward through zero or more `]['key']`
    //    or `]["key"]` patterns, collecting the keys.
    let mut prefix_keys: Vec<String> = Vec::new();
    loop {
        // We're now right before the `[` we just consumed.
        // Check if there is a preceding `]` — that would indicate a
        // chained access like `$var['k1']['k2'][`.
        if i == 0 || chars[i - 1] != ']' {
            break;
        }
        // Try to parse the preceding `['key']` segment.
        let saved_i = i;
        i -= 1; // skip `]`

        // Expect a closing quote.
        if i == 0 || (chars[i - 1] != '\'' && chars[i - 1] != '"') {
            i = saved_i;
            break;
        }
        let prev_quote = chars[i - 1];
        i -= 1; // skip closing quote

        // Collect the key text (walk backward to the matching opening quote).
        let key_end = i;
        while i > 0 && chars[i - 1] != prev_quote {
            i -= 1;
        }
        if i == 0 {
            i = saved_i;
            break;
        }
        let key_text: String = chars[i..key_end].iter().collect();
        i -= 1; // skip opening quote

        // Expect `[` before the opening quote.
        if i == 0 || chars[i - 1] != '[' {
            i = saved_i;
            break;
        }
        i -= 1; // skip `[`

        prefix_keys.push(key_text);
    }

    // Reverse so prefix_keys[0] is the outermost key.
    prefix_keys.reverse();

    // 5. Extract the variable name before the first `[`.
    //    Walk backward through identifier chars, then expect `$`.
    let bracket_pos = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }
    if i == 0 || chars[i - 1] != '$' {
        return None;
    }
    i -= 1; // include `$`

    let var_name: String = chars[i..bracket_pos].iter().collect();
    if var_name.len() < 2 {
        // Must be at least `$x`
        return None;
    }

    let partial_key: String = chars[partial_start..partial_end].iter().collect();

    Some(ArrayKeyContext {
        var_name,
        partial_key,
        quote_char,
        key_start_col,
        prefix_keys,
    })
}

impl Backend {
    /// Build completion items for array shape keys.
    ///
    /// Given an `ArrayKeyContext`, resolves the variable's type annotation
    /// and, if it is an array shape, returns completion items for each key.
    ///
    /// Uses `text_edit` with a range that covers any auto-inserted trailing
    /// characters (closing quote + `]`) so that accepting a completion does
    /// not produce duplicate brackets.
    pub(crate) fn build_array_key_completions(
        &self,
        ctx: &ArrayKeyContext,
        content: &str,
        position: Position,
        file_ctx: &FileContext,
    ) -> Vec<CompletionItem> {
        // ── $_SERVER superglobal — hardcoded keys ────────────────────
        if ctx.var_name == "$_SERVER" && ctx.prefix_keys.is_empty() {
            return self.build_server_key_completions(ctx, content, position);
        }

        let cursor_offset = position_to_offset(content, position);

        // Try to find the raw type annotation for this variable.
        // We also track which set of classes was used for resolution so
        // that type alias expansion can consult the same set (important
        // when the original parse fails and patched classes are used).
        let raw_type = self.resolve_variable_raw_type(
            &ctx.var_name,
            content,
            cursor_offset as usize,
            file_ctx,
        );

        // If initial resolution failed, the content likely has a syntax
        // error (e.g. unclosed `$var['`) that prevented the parser from
        // recovering the class structure.  Patch the cursor line to close
        // the array access, re-parse, and retry.
        let patched_classes_storage;
        let (raw_type, effective_classes) = match raw_type {
            Some(t) => (t, file_ctx.classes.as_slice()),
            None => {
                let patched = patch_array_access_at_cursor(content, position);
                if patched == content {
                    return vec![];
                }
                patched_classes_storage = self
                    .parse_php(&patched)
                    .into_iter()
                    .map(Arc::new)
                    .collect::<Vec<_>>();
                let patched_offset = position_to_offset(&patched, position);
                let patched_ctx = FileContext {
                    classes: patched_classes_storage.clone(),
                    use_map: file_ctx.use_map.clone(),
                    namespace: file_ctx.namespace.clone(),
                    resolved_names: file_ctx.resolved_names.clone(),
                };
                match self.resolve_variable_raw_type(
                    &ctx.var_name,
                    &patched,
                    patched_offset as usize,
                    &patched_ctx,
                ) {
                    Some(t) => (t, patched_classes_storage.as_slice()),
                    None => return vec![],
                }
            }
        };

        // If there are prefix keys (chained access), resolve through each
        // level of the shape to get the inner type.
        let effective_type = self.resolve_through_prefix_keys(&raw_type, &ctx.prefix_keys);
        let effective_type = match effective_type {
            Some(t) => t,
            None => return vec![],
        };

        // Expand type aliases before parsing as an array shape.
        // The raw type might be an alias name like `UserData` that
        // resolves to `array{name: string, email: string}`.
        // Uses `effective_classes` which may be the patched classes when
        // the original parse failed due to syntax errors.
        let class_loader =
            self.class_loader_with(effective_classes, &file_ctx.use_map, &file_ctx.namespace);
        let parsed = super::type_resolution::resolve_type_alias_typed(
            &effective_type,
            "",
            effective_classes,
            &class_loader,
        )
        .unwrap_or(effective_type);
        let entries = match parsed.shape_entries() {
            Some(e) => e,
            None => return vec![],
        };

        // Compute the text edit range that covers the partial key and any
        // trailing auto-inserted characters after the cursor.
        let (range, _) = self.compute_edit_range(ctx, content, position);

        // Build completion items, filtering by partial key.
        let quote = ctx.quote_char.unwrap_or('\'');
        let mut items = Vec::new();

        for (sort_idx, entry) in entries.iter().enumerate() {
            let key_name = match entry.key.as_deref() {
                Some(k) => k,
                None => continue, // skip positional entries without explicit keys
            };

            // Filter by partial key prefix.
            if !ctx.partial_key.is_empty()
                && !key_name
                    .to_lowercase()
                    .starts_with(&ctx.partial_key.to_lowercase())
            {
                continue;
            }

            let optional_marker = if entry.optional { "?" } else { "" };
            let detail = format!("{}{}: {}", key_name, optional_marker, entry.value_type);

            // The new_text always produces the complete `key']` or `'key']`
            // fragment. The text_edit range is set to cover any existing
            // partial key text *and* any trailing auto-closed chars, so
            // accepting the completion replaces everything cleanly.
            let new_text = if ctx.quote_char.is_some() {
                format!("{}{}]", key_name, quote)
            } else {
                format!("{}{}{}]", quote, key_name, quote)
            };

            items.push(CompletionItem {
                label: key_name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(detail),
                filter_text: Some(key_name.to_string()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
                sort_text: Some(format!("{:04}", sort_idx)),
                ..CompletionItem::default()
            });
        }

        items
    }

    /// Build completion items for `$_SERVER` superglobal keys.
    fn build_server_key_completions(
        &self,
        ctx: &ArrayKeyContext,
        content: &str,
        position: Position,
    ) -> Vec<CompletionItem> {
        let (range, _) = self.compute_edit_range(ctx, content, position);
        let quote = ctx.quote_char.unwrap_or('\'');
        let mut items = Vec::new();

        for (sort_idx, &(key, detail)) in SERVER_KEYS.iter().enumerate() {
            // Filter by partial key prefix.
            if !ctx.partial_key.is_empty()
                && !key
                    .to_lowercase()
                    .starts_with(&ctx.partial_key.to_lowercase())
            {
                continue;
            }

            let new_text = if ctx.quote_char.is_some() {
                format!("{}{}]", key, quote)
            } else {
                format!("{}{}{}]", quote, key, quote)
            };

            items.push(CompletionItem {
                label: key.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(detail.to_string()),
                filter_text: Some(key.to_string()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
                sort_text: Some(format!("{:04}", sort_idx)),
                ..CompletionItem::default()
            });
        }

        items
    }

    /// Compute the `TextEdit` range for an array key completion.
    ///
    /// The range starts at `key_start_col` (right after the opening quote
    /// or `[`) and extends past any trailing auto-inserted characters
    /// (closing quote + `]`) that the IDE may have inserted.
    ///
    /// Returns `(range, trailing_count)` where `trailing_count` is the
    /// number of characters consumed after the cursor.
    fn compute_edit_range(
        &self,
        ctx: &ArrayKeyContext,
        content: &str,
        position: Position,
    ) -> (Range, usize) {
        let lines: Vec<&str> = content.lines().collect();
        let line_idx = position.line as usize;
        let trailing_count = if line_idx < lines.len() {
            let line = lines[line_idx];
            let chars: Vec<char> = line.chars().collect();
            let cursor_col = position.character as usize;
            count_trailing_close_chars(&chars, cursor_col, ctx.quote_char)
        } else {
            0
        };

        let range = Range {
            start: Position {
                line: position.line,
                character: ctx.key_start_col,
            },
            end: Position {
                line: position.line,
                character: position.character + trailing_count as u32,
            },
        };

        (range, trailing_count)
    }

    /// Walk through `prefix_keys` to resolve the inner type of a nested
    /// array shape.
    ///
    /// Given a raw type like `array{meta: array{page: int, total: int}}` and
    /// prefix keys `["meta"]`, returns `Some(PhpType)` for `array{page: int, total: int}`.
    fn resolve_through_prefix_keys(
        &self,
        raw_type: &PhpType,
        prefix_keys: &[String],
    ) -> Option<PhpType> {
        if prefix_keys.is_empty() {
            return Some(raw_type.clone());
        }

        let mut current = raw_type.clone();
        for key in prefix_keys {
            current = current.shape_value_type(key)?.clone();
        }
        Some(current)
    }

    /// Resolve the raw (uncleaned) type annotation for a variable.
    ///
    /// Searches for `@var` and `@param` annotations, and also follows
    /// simple assignments from function/method calls to extract their
    /// `@return` type.
    ///
    /// Returns the raw type (e.g. `array{name: string, user: User}`)
    /// or `None` if no type annotation is found.
    pub(crate) fn resolve_variable_raw_type(
        &self,
        var_name: &str,
        content: &str,
        cursor_offset: usize,
        file_ctx: &FileContext,
    ) -> Option<PhpType> {
        // 1. Direct @var / @param annotation on the variable.
        let current_class = find_class_at_offset(&file_ctx.classes, cursor_offset as u32);
        let class_loader = self.class_loader(file_ctx);

        if let Some(raw) =
            docblock::find_iterable_raw_type_in_source(content, cursor_offset, var_name)
        {
            return Some(crate::util::resolve_php_type_names(&raw, &class_loader));
        }

        // 2. Unified variable resolver — handles array literals with
        //    incremental key assignments, push-style assignments, and
        //    standalone function return types (via source docblock scan).
        let dummy_class;
        let effective_class = match current_class {
            Some(cc) => cc,
            None => {
                dummy_class = ClassInfo::default();
                &dummy_class
            }
        };
        let function_loader = self.function_loader(file_ctx);
        let resolved = crate::completion::variable::resolution::resolve_variable_types(
            var_name,
            effective_class,
            &file_ctx.classes,
            content,
            cursor_offset as u32,
            &class_loader,
            Loaders {
                function_loader: Some(&function_loader),
                ..Loaders::default()
            },
        );
        if resolved.is_empty() {
            None
        } else {
            Some(ResolvedType::types_joined(&resolved))
        }
    }
}

/// Patch the content at the cursor line to close an unclosed array key
/// access so that the PHP parser can recover the surrounding class/function
/// structure.
///
/// Replaces patterns like `$var['` or `$var[` at the cursor line with
/// `$var[''];` (or `$var[];`) so the rest of the file parses correctly.
fn patch_array_access_at_cursor(content: &str, position: Position) -> String {
    let line_idx = position.line as usize;
    let mut result = String::with_capacity(content.len() + 4);

    for (i, line) in content.lines().enumerate() {
        if i == line_idx {
            let trimmed = line.trim_end();
            // Detect the unclosed pattern and close it.
            // Order matters: check longer/more-specific patterns first so
            // that e.g. `['']` is not partially matched by `['`.
            if trimmed.ends_with("['']") || trimmed.ends_with("[\"\"]") {
                // Fully auto-closed quotes + bracket — just add semicolon.
                result.push_str(trimmed);
                result.push(';');
            } else if trimmed.ends_with("[']") || trimmed.ends_with("[\"]") {
                // Quote + auto-closed bracket without closing quote:
                //   `$data[']` → `$data[''];`
                //   `$data["]` → `$data[""];`
                let q = if trimmed.ends_with("[']") { '\'' } else { '"' };
                // Insert the closing quote before the `]`.
                let before_bracket = &trimmed[..trimmed.len() - 1];
                result.push_str(before_bracket);
                result.push(q);
                result.push_str("];");
            } else if trimmed.ends_with("['") || trimmed.ends_with("[\"") {
                result.push_str(trimmed);
                // Close the quote + bracket + semicolon
                let q = if trimmed.ends_with("['") { '\'' } else { '"' };
                result.push(q);
                result.push_str("];");
            } else if trimmed.ends_with("[]") {
                result.push_str(trimmed);
                result.push(';');
            } else if trimmed.ends_with('[') {
                result.push_str(trimmed);
                result.push_str("];");
            } else {
                // Nothing to patch
                result.push_str(line);
            }
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // Remove trailing newline if the original didn't end with one.
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Count the number of trailing auto-inserted characters after the cursor.
///
/// When the IDE auto-closes brackets, the line may contain:
///   - `']` or `"]` after the cursor (2 chars) — when a quote was typed
///   - `]` after the cursor (1 char) — when only `[` was typed
///
/// This function looks at the characters starting at `cursor_col` and
/// returns how many should be consumed by the text edit range.
fn count_trailing_close_chars(
    chars: &[char],
    cursor_col: usize,
    quote_char: Option<char>,
) -> usize {
    if cursor_col >= chars.len() {
        return 0;
    }

    let remaining = &chars[cursor_col..];

    match quote_char {
        Some(q) => {
            // Expect closing quote + `]`
            if remaining.len() >= 2 && remaining[0] == q && remaining[1] == ']' {
                2
            } else if !remaining.is_empty() && remaining[0] == ']' {
                // Just a `]` even though we had a quote — still consume it
                1
            } else {
                0
            }
        }
        None => {
            // Expect just `]`
            if !remaining.is_empty() && remaining[0] == ']' {
                1
            } else {
                0
            }
        }
    }
}

/// Extract spread expressions from an array literal.
///
/// Given an array literal like `[...$users, 'key' => 'val', ...$admins]`,
/// this returns `Some(vec!["$users", "$admins"])`.
///
/// Only elements that start with `...` are collected.  Keyed entries and
/// non-spread positional entries are ignored.
///
/// Returns `None` if `rhs` is not an array literal, or `Some(vec![])` if
/// it is an array literal but contains no spread elements.
pub fn extract_spread_expressions(rhs: &str) -> Option<Vec<String>> {
    let inner = if rhs.starts_with('[') && rhs.ends_with(']') {
        &rhs[1..rhs.len() - 1]
    } else {
        let lower = rhs.to_ascii_lowercase();
        if lower.starts_with("array(") && rhs.ends_with(')') {
            &rhs[6..rhs.len() - 1]
        } else {
            return None;
        }
    };

    let inner = inner.trim();
    if inner.is_empty() {
        return Some(vec![]);
    }

    let parts = split_array_literal_elements(inner);
    let mut spreads = Vec::new();

    for part in &parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(expr) = part.strip_prefix("...") {
            let expr = expr.trim();
            if !expr.is_empty() {
                spreads.push(expr.to_string());
            }
        }
    }

    Some(spreads)
}

/// Split array literal elements on commas at depth 0, respecting
/// `(…)`, `[…]`, `{…}`, `<…>` nesting and quoted strings.
fn split_array_literal_elements(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev_char = '\0';
    let mut start = 0;

    for (i, ch) in s.char_indices() {
        if in_single_quote {
            if ch == '\'' && prev_char != '\\' {
                in_single_quote = false;
            }
            prev_char = ch;
            continue;
        }
        if in_double_quote {
            if ch == '"' && prev_char != '\\' {
                in_double_quote = false;
            }
            prev_char = ch;
            continue;
        }

        match ch {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '(' | '[' => {
                if ch == '(' {
                    depth_paren += 1;
                } else {
                    depth_bracket += 1;
                }
            }
            ')' => depth_paren -= 1,
            ']' => depth_bracket -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        prev_char = ch;
    }
    let last = &s[start..];
    if !last.trim().is_empty() {
        parts.push(last);
    }
    parts
}

#[cfg(test)]
#[path = "array_shape_tests.rs"]
mod tests;
