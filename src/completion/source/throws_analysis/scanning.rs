//! Core scanning primitives and method-lookup helpers for throws analysis.
//!
//! These byte-level scanners are shared by [`super::catch`] and
//! [`super::cross_file`]:
//!   - Find `throw new Type(…)` statements in a block of PHP code
//!   - Find `throw $this->method(…)` / `throw self::method(…)` patterns
//!     and resolve the called method's return type
//!   - Find method calls and collect their `@throws` docblock annotations
//!   - Look up a method's return type from its declaration or docblock
//!   - Look up a method's `@throws` tags from its docblock
//!   - Extract the function body following a docblock

use crate::completion::source::comment_position::position_to_byte_offset;
use crate::php_type::PhpType;
use crate::text_scan::{
    find_matching_delimiter_forward, skip_block_comment, skip_line_comment, skip_string_forward,
};
use tower_lsp::lsp_types::Position;

/// Information about a `throw` statement (or throw-expression) found in
/// a block of PHP source code.
#[derive(Debug)]
pub(crate) struct ThrowInfo {
    /// The exception type as written in source (e.g.
    /// `PhpType::Named("InvalidArgumentException")`,
    /// `PhpType::Named("RuntimeException")`).
    pub type_name: PhpType,
    /// Byte offset of this throw statement relative to the start of the
    /// scanned block.
    pub offset: usize,
}

// ─── Core Scanning Primitives ───────────────────────────────────────────────

/// Find all `throw new Type(…)` statements in the given PHP source text.
///
/// Returns a [`ThrowInfo`] for each statement with the type name and the
/// byte offset of the `throw` keyword within `body`.
pub(crate) fn find_throw_statements(body: &str) -> Vec<ThrowInfo> {
    let mut results = Vec::new();
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Skip string literals
        if bytes[pos] == b'\'' || bytes[pos] == b'"' {
            pos = skip_string_forward(bytes, pos);
            continue;
        }

        // Skip line comments
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            pos = skip_line_comment(bytes, pos);
            continue;
        }

        // Skip block comments
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos = skip_block_comment(bytes, pos);
            continue;
        }

        // Look for `throw` keyword
        if pos + 5 <= len && &body[pos..pos + 5] == "throw" {
            let before_ok =
                pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric() && bytes[pos - 1] != b'_';
            let after_ok = pos + 5 >= len
                || (!bytes[pos + 5].is_ascii_alphanumeric() && bytes[pos + 5] != b'_');
            if before_ok && after_ok {
                let after_throw = body[pos + 5..].trim_start();
                if after_throw.starts_with("new ")
                    || after_throw.starts_with("new\t")
                    || after_throw.starts_with("new\n")
                {
                    let after_new = after_throw[3..].trim_start();
                    let type_end = after_new
                        .find(|c: char| !c.is_alphanumeric() && c != '\\' && c != '_')
                        .unwrap_or(after_new.len());
                    let type_name = &after_new[..type_end];
                    if !type_name.is_empty() {
                        results.push(ThrowInfo {
                            type_name: PhpType::Named(type_name.to_string()),
                            offset: pos,
                        });
                    }
                }
            }
        }

        pos += 1;
    }

    results
}

/// Find `throw $this->method(…)` / `throw self::method(…)` /
/// `throw static::method(…)` patterns and resolve the called method's
/// return type from its declaration or docblock in the same file.
///
/// Returns a [`ThrowInfo`] for each resolved throw-expression.
pub(crate) fn find_throw_expression_types(body: &str, file_content: &str) -> Vec<ThrowInfo> {
    let mut results = Vec::new();
    let method_patterns: &[&str] = &["$this->", "self::", "static::"];

    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        if bytes[pos] == b'\'' || bytes[pos] == b'"' {
            pos = skip_string_forward(bytes, pos);
            continue;
        }
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            pos = skip_line_comment(bytes, pos);
            continue;
        }
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos = skip_block_comment(bytes, pos);
            continue;
        }

        // Look for `throw` keyword
        if pos + 5 <= len && &body[pos..pos + 5] == "throw" {
            let before_ok =
                pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric() && bytes[pos - 1] != b'_';
            let after_ok = pos + 5 >= len
                || (!bytes[pos + 5].is_ascii_alphanumeric() && bytes[pos + 5] != b'_');
            if before_ok && after_ok {
                let after_throw = body[pos + 5..].trim_start();
                // Skip `throw new` (handled by find_throw_statements)
                let is_new = after_throw.starts_with("new ")
                    || after_throw.starts_with("new\t")
                    || after_throw.starts_with("new\n");
                if !is_new {
                    let mut matched = false;
                    // Try method-call patterns first: $this->m(), self::m(), static::m()
                    for pat in method_patterns {
                        if let Some(rest) = after_throw.strip_prefix(pat) {
                            let name_end = rest
                                .find(|c: char| !c.is_alphanumeric() && c != '_')
                                .unwrap_or(rest.len());
                            let method_name = &rest[..name_end];
                            if !method_name.is_empty()
                                && let Some(ret_type) =
                                    find_method_return_type(file_content, method_name)
                            {
                                results.push(ThrowInfo {
                                    type_name: ret_type,
                                    offset: pos,
                                });
                            }
                            matched = true;
                            break;
                        }
                    }
                    // Bare function call: `throw makeException(…)`
                    if !matched {
                        let name_end = after_throw
                            .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '\\')
                            .unwrap_or(after_throw.len());
                        let func_name = after_throw[..name_end].trim_start_matches('\\');
                        let after_name = after_throw[name_end..].trim_start();
                        if !func_name.is_empty()
                            && after_name.starts_with('(')
                            && let Some(ret_type) = find_method_return_type(file_content, func_name)
                        {
                            results.push(ThrowInfo {
                                type_name: ret_type,
                                offset: pos,
                            });
                        }
                    }
                }
            }
        }

        pos += 1;
    }

    results
}

/// Find all method calls (`$this->method(…)`, `self::method(…)`,
/// `static::method(…)`) in the given PHP source text and collect
/// `@throws` annotations from those methods' docblocks in the same file.
///
/// This propagates `@throws` declarations: if method A calls method B
/// and B declares `@throws SomeException`, then A should also be aware
/// of that exception.
///
/// Returns a [`ThrowInfo`] for each propagated throw, with the byte
/// offset set to the call site so that catch-block filtering works.
pub(crate) fn find_propagated_throws(body: &str, file_content: &str) -> Vec<ThrowInfo> {
    let mut results = Vec::new();
    let mut seen_methods = std::collections::HashSet::new();
    let patterns: &[&str] = &["$this->", "self::", "static::"];

    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        if bytes[pos] == b'\'' || bytes[pos] == b'"' {
            pos = skip_string_forward(bytes, pos);
            continue;
        }
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            pos = skip_line_comment(bytes, pos);
            continue;
        }
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos = skip_block_comment(bytes, pos);
            continue;
        }

        for pat in patterns {
            if pos + pat.len() <= len && &body[pos..pos + pat.len()] == *pat {
                let before_ok = if *pat == "$this->" {
                    true
                } else {
                    pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric() && bytes[pos - 1] != b'_'
                };
                if !before_ok {
                    break;
                }

                let after_pat = &body[pos + pat.len()..];
                let name_end = after_pat
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(after_pat.len());
                let method_name = &after_pat[..name_end];

                let after_name = after_pat[name_end..].trim_start();
                if !method_name.is_empty()
                    && after_name.starts_with('(')
                    && seen_methods.insert(method_name.to_string())
                {
                    let throws = find_method_throws_tags(file_content, method_name);
                    for t in throws {
                        results.push(ThrowInfo {
                            type_name: t,
                            offset: pos,
                        });
                    }
                }
                break;
            }
        }

        pos += 1;
    }

    results
}

/// Find inline `/** @throws ExceptionType */` annotations in a block of
/// PHP code.
///
/// These are single-line docblock comments that developers place inside
/// code (often in a try block) to hint at exceptions thrown by code that
/// doesn't have `@throws` annotations itself.
///
/// Returns the short type names found.
pub(crate) fn find_inline_throws_annotations(body: &str) -> Vec<ThrowInfo> {
    let mut results = Vec::new();
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos + 6 < len {
        // Look for `/**`
        if bytes[pos] == b'/' && pos + 2 < len && bytes[pos + 1] == b'*' && bytes[pos + 2] == b'*' {
            let doc_start = pos;
            pos += 3;

            // Find the closing `*/`
            let mut doc_end = None;
            while pos + 1 < len {
                if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                    doc_end = Some(pos + 2);
                    break;
                }
                pos += 1;
            }

            if let Some(end) = doc_end {
                let docblock = &body[doc_start..end];
                if let Some(info) = crate::docblock::parser::parse_docblock_for_tags(docblock) {
                    use mago_docblock::document::TagKind;
                    for tag in info.tags_by_kind(TagKind::Throws) {
                        let rest = tag.description.trim();
                        if let Some(type_name) = rest.split_whitespace().next() {
                            let clean = type_name
                                .trim_start_matches('\\')
                                .trim_end_matches('*')
                                .trim_end_matches('/');
                            if !clean.is_empty() && !clean.starts_with('$') {
                                results.push(ThrowInfo {
                                    type_name: PhpType::Named(clean.to_string()),
                                    offset: doc_start,
                                });
                            }
                        }
                    }
                }
                pos = end;
                continue;
            }
        }

        pos += 1;
    }

    results
}

// ─── Method Lookup Helpers ──────────────────────────────────────────────────

/// Find the return type of a method by scanning the file content for its
/// declaration.
///
/// Checks the native return type hint first, then falls back to the
/// `@return` tag in the method's docblock.  Skips visibility and
/// modifier keywords between the docblock and the `function` keyword.
///
/// Returns the short type name (last segment after `\`), or `None` if
/// the method is not found or has no resolvable return type.
pub(crate) fn find_method_return_type(file_content: &str, method_name: &str) -> Option<PhpType> {
    let search = format!("function {}", method_name);

    let mut search_start = 0;
    while let Some(func_pos) = file_content[search_start..].find(&search) {
        let abs_pos = search_start + func_pos;
        search_start = abs_pos + search.len();

        let after_pos = abs_pos + search.len();
        if after_pos < file_content.len() {
            let next_byte = file_content.as_bytes()[after_pos];
            if next_byte.is_ascii_alphanumeric() || next_byte == b'_' {
                continue;
            }
        }

        // Check the native return type: find matching `)` then `: Type`
        let after = &file_content[after_pos..];
        if let Some(paren_start) = after.find('(')
            && let Some(close_offset) =
                find_matching_delimiter_forward(after, paren_start, b'(', b')')
        {
            let after_close = after[close_offset + 1..].trim_start();
            if let Some(rest) = after_close.strip_prefix(':') {
                let rest = rest.trim_start();
                let type_end = rest.find(['{', ';']).unwrap_or(rest.len());
                let type_str = rest[..type_end].trim();
                if !type_str.is_empty() {
                    let parsed = PhpType::parse(type_str);
                    let non_null = parsed.non_null_type().unwrap_or_else(|| parsed.clone());
                    if let Some(name) = non_null.base_name() {
                        return Some(PhpType::Named(name.to_string()));
                    }
                }
            }
        }

        // Check docblock @return, skipping visibility/modifier keywords
        let before = skip_modifiers_backward(&file_content[..abs_pos]);
        if before.ends_with("*/")
            && let Some(doc_start) = before.rfind("/**")
        {
            let docblock = &before[doc_start..];
            if let Some(info) = crate::docblock::parser::parse_docblock_for_tags(docblock) {
                use mago_docblock::document::TagKind;
                if let Some(tag) = info.first_tag_by_kind(TagKind::Return) {
                    let rest = tag.description.trim();
                    if let Some(type_str) = rest.split_whitespace().next() {
                        let parsed = PhpType::parse(type_str);
                        let non_null = parsed.non_null_type().unwrap_or_else(|| parsed.clone());
                        if let Some(name) = non_null.base_name()
                            && !parsed.is_void()
                            && !parsed.is_mixed()
                            && !parsed.is_self_like()
                        {
                            return Some(PhpType::Named(name.to_string()));
                        }
                    }
                }
            }
        }
        break;
    }

    None
}

/// Find `@throws` tags in a method's docblock by scanning the file
/// content for the method declaration.
///
/// Skips visibility and modifier keywords between the docblock and the
/// `function` keyword.
///
/// Returns the short type names declared in `@throws` tags.
pub(crate) fn find_method_throws_tags(file_content: &str, method_name: &str) -> Vec<PhpType> {
    let mut throws = Vec::new();
    let search = format!("function {}", method_name);

    let mut search_start = 0;
    while let Some(func_pos) = file_content[search_start..].find(&search) {
        let abs_pos = search_start + func_pos;
        search_start = abs_pos + search.len();

        // Verify word boundary after
        let after_pos = abs_pos + search.len();
        if after_pos < file_content.len() {
            let next_byte = file_content.as_bytes()[after_pos];
            if next_byte.is_ascii_alphanumeric() || next_byte == b'_' {
                continue;
            }
        }

        // Look backward for a docblock, skipping visibility/modifier keywords
        let before = skip_modifiers_backward(&file_content[..abs_pos]);
        if before.ends_with("*/")
            && let Some(doc_start) = before.rfind("/**")
        {
            let docblock = &before[doc_start..];
            if let Some(info) = crate::docblock::parser::parse_docblock_for_tags(docblock) {
                use mago_docblock::document::TagKind;
                for tag in info.tags_by_kind(TagKind::Throws) {
                    let rest = tag.description.trim();
                    if let Some(type_str) = rest.split_whitespace().next() {
                        let clean = type_str
                            .trim_end_matches('/')
                            .trim_end_matches('*')
                            .trim_start_matches('\\');
                        if !clean.is_empty() {
                            throws.push(PhpType::Named(clean.to_string()));
                        }
                    }
                }
            }
        }
        break;
    }

    throws
}

// ─── Internal Helpers ───────────────────────────────────────────────────────

/// Skip backward past PHP visibility and modifier keywords
/// (`public`, `protected`, `private`, `static`, `abstract`, `final`,
/// `readonly`) to locate the docblock that precedes a method
/// declaration.
///
/// Returns the trimmed prefix of `text` with modifiers stripped.
pub(crate) fn skip_modifiers_backward(text: &str) -> &str {
    const MODIFIERS: &[&str] = &[
        "private",
        "protected",
        "public",
        "static",
        "abstract",
        "final",
        "readonly",
    ];

    let mut s = text.trim_end();
    loop {
        let mut found = false;
        for modifier in MODIFIERS {
            if s.ends_with(modifier) {
                let start = s.len() - modifier.len();
                if start == 0
                    || (!s.as_bytes()[start - 1].is_ascii_alphanumeric()
                        && s.as_bytes()[start - 1] != b'_')
                {
                    s = s[..start].trim_end();
                    found = true;
                    break;
                }
            }
        }
        if !found {
            break;
        }
    }
    s
}

// ─── Function Body Extraction ───────────────────────────────────────────────

/// Extract the function/method body text that follows the docblock at
/// the cursor position.
///
/// Returns the text between the opening `{` and matching closing `}` of
/// the function/method declaration.  Returns `None` if the body cannot
/// be located (e.g. abstract method, or the docblock is not followed by
/// a function).
pub(crate) fn extract_function_body(content: &str, position: Position) -> Option<String> {
    let after_docblock = {
        let byte_offset = position_to_byte_offset(content, position);
        let after_cursor = &content[byte_offset.min(content.len())..];

        if let Some(close_pos) = after_cursor.find("*/") {
            after_cursor[close_pos + 2..].to_string()
        } else {
            after_cursor.to_string()
        }
    };

    // Find the `function` keyword to confirm this is a function/method.
    let func_idx = {
        let lower = after_docblock.to_lowercase();
        let mut start = 0;
        let mut found = None;
        while let Some(pos) = lower[start..].find("function") {
            let abs = start + pos;
            let before_ok = abs == 0 || !after_docblock.as_bytes()[abs - 1].is_ascii_alphanumeric();
            let after_pos = abs + 8; // "function".len()
            let after_ok = after_pos >= after_docblock.len()
                || !after_docblock.as_bytes()[after_pos].is_ascii_alphanumeric();
            if before_ok && after_ok {
                found = Some(abs);
                break;
            }
            start = abs + 8;
        }
        found?
    };

    let after_func = &after_docblock[func_idx..];

    // Find the opening brace of the function body.
    let open_brace = after_func.find('{')?;
    let body_start = open_brace + 1;

    // Walk forward to find the matching closing brace.
    let mut depth = 1u32;
    let mut pos = body_start;
    let bytes = after_func.as_bytes();
    // Track whether we are inside a string literal to avoid counting
    // braces inside strings.
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    while pos < bytes.len() && depth > 0 {
        let b = bytes[pos];
        if in_single_quote {
            if b == b'\\' {
                pos += 1; // skip escaped char
            } else if b == b'\'' {
                in_single_quote = false;
            }
        } else if in_double_quote {
            if b == b'\\' {
                pos += 1; // skip escaped char
            } else if b == b'"' {
                in_double_quote = false;
            }
        } else {
            match b {
                b'\'' => in_single_quote = true,
                b'"' => in_double_quote = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(after_func[body_start..pos].to_string());
                    }
                }
                b'/' if pos + 1 < bytes.len() => {
                    // Skip line comments
                    if bytes[pos + 1] == b'/' {
                        while pos < bytes.len() && bytes[pos] != b'\n' {
                            pos += 1;
                        }
                        continue;
                    }
                    // Skip block comments
                    if bytes[pos + 1] == b'*' {
                        pos += 2;
                        while pos + 1 < bytes.len() {
                            if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                                pos += 1;
                                break;
                            }
                            pos += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        pos += 1;
    }

    None
}
