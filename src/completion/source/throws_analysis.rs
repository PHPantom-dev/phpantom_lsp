//! Throws analysis: scanning, catch-block filtering, and uncaught detection.
//!
//! This module provides a complete throws-analysis pipeline used by both
//! `phpdoc.rs` (for `@throws` tag completion) and `catch_completion.rs`
//! (for catch-clause exception suggestions).
//!
//! **Low-level scanning** (used by both callers):
//!   - Find `throw new Type(…)` statements in a block of PHP code
//!   - Find `throw $this->method(…)` / `throw self::method(…)` patterns
//!     and resolve the called method's return type
//!   - Find method calls and collect their `@throws` docblock annotations
//!   - Look up a method's return type from its declaration or docblock
//!   - Look up a method's `@throws` tags from its docblock
//!
//! **High-level analysis** (used by phpdoc completion):
//!   - Extract the function body following a docblock
//!   - Find `try/catch` blocks and their caught exception types
//!   - Determine which thrown exceptions are **not** caught
//!   - Resolve exception short names to FQNs via use-map / namespace
//!   - Check whether a `use` import already exists in the file
//!
//! Callers that only need type names can map `ThrowInfo::type_name`;
//! callers that need offset information (e.g. for catch-block filtering)
//! use the full `ThrowInfo` struct.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::Position;

use super::comment_position::position_to_byte_offset;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionLoader};

/// Optional reference to a class-loader closure, used by nested
/// helper functions in throws analysis.
type OptClassLoader<'a> = Option<&'a dyn Fn(&str) -> Option<Arc<ClassInfo>>>;

/// Bundles the loaders needed for cross-file throws resolution.
///
/// When provided to [`find_uncaught_throw_types_with_context`], every call
/// in the function body is inspected:
///
/// - `$variable->method()` — the variable's type is resolved from the
///   function's parameter list, the class is loaded, and the method's
///   `@throws` tags are propagated.
/// - `ClassName::staticMethod()` — the class is loaded directly and the
///   method's `@throws` tags are propagated.
/// - `functionName()` — the function is loaded and its `@throws` tags
///   are propagated.
/// - `new ClassName(…)` — the class is loaded and the constructor's
///   `@throws` tags are propagated.
pub(crate) struct ThrowsContext<'a> {
    /// Resolves a class name to its [`ClassInfo`].
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Resolves a function name to its [`FunctionInfo`].
    pub function_loader: FunctionLoader<'a>,
    /// Use-statement map for the current file (short name → FQN).
    /// Used to resolve short exception names to fully-qualified names.
    pub use_map: &'a HashMap<String, String>,
    /// The namespace of the current file, if any.
    pub file_namespace: &'a Option<String>,
}

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

/// Find `throw $variable` patterns and resolve the variable's exception
/// type from catch clauses whose body contains the throw.
///
/// When `throw $e` appears inside a `catch (SomeException $e) { … }` block,
/// the thrown type is `SomeException`.
fn find_throw_variable_types(body: &str, catches: &[CatchInfo]) -> Vec<ThrowInfo> {
    let mut results = Vec::new();
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
                if after_throw.starts_with('$') {
                    // Extract the variable name (e.g. `$e`)
                    let var_end = after_throw
                        .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(after_throw.len());
                    let var_name = &after_throw[..var_end];
                    if var_name.len() > 1 {
                        // Find which catch clause this throw lives in and
                        // whose variable matches.
                        for c in catches {
                            if pos > c.catch_body_start
                                && pos < c.catch_body_end
                                && c.var_name.as_deref() == Some(var_name)
                            {
                                for tn in &c.type_names {
                                    results.push(ThrowInfo {
                                        type_name: tn.clone(),
                                        offset: pos,
                                    });
                                }
                                break;
                            }
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
fn skip_modifiers_backward(text: &str) -> &str {
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

/// Find the matching closing delimiter for an opening delimiter at
/// `open_pos`, respecting string literal nesting.
///
/// `open` and `close` are the delimiter bytes (e.g. `b'('` / `b')'`
/// or `b'{'` / `b'}'`).
fn find_matching_delimiter_forward(
    text: &str,
    open_pos: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = text.as_bytes();
    if open_pos >= bytes.len() || bytes[open_pos] != open {
        return None;
    }

    let mut depth = 1i32;
    let mut pos = open_pos + 1;

    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            b'\'' | b'"' => {
                let quote = bytes[pos];
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' {
                        pos += 1;
                    } else if bytes[pos] == quote {
                        break;
                    }
                    pos += 1;
                }
            }
            _ => {}
        }
        pos += 1;
    }

    None
}

/// Skip past a string literal starting at `pos` (which must point to
/// the opening quote).  Returns the position after the closing quote.
fn skip_string_forward(bytes: &[u8], pos: usize) -> usize {
    let quote = bytes[pos];
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1; // skip escaped char
        } else if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Skip past a line comment (`//…`) starting at `pos`.  Returns the
/// position of the newline (or end of input).
fn skip_line_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Skip past a block comment (`/* … */`) starting at `pos`.  Returns
/// the position after the closing `*/` (or end of input).
fn skip_block_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    i
}

// ─── High-Level Uncaught Throws Analysis ────────────────────────────────────

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

/// Information about a `catch (Type $var)` clause in a function body.
#[derive(Debug)]
struct CatchInfo {
    /// The caught exception type names (multi-catch produces multiple).
    type_names: Vec<PhpType>,
    /// The variable name from the catch clause (e.g. `"$e"`), if present.
    var_name: Option<String>,
    /// Byte offset of the start of the `try` block this catch belongs to.
    try_start: usize,
    /// Byte offset of the end of the `try` block (the matching `}`).
    try_end: usize,
    /// Byte offset of the opening `{` of the catch block body.
    catch_body_start: usize,
    /// Byte offset of the closing `}` of the catch block body.
    catch_body_end: usize,
}

/// Find all `try { … } catch (…)` blocks and their caught types.
fn find_catch_blocks(body: &str) -> Vec<CatchInfo> {
    let mut results = Vec::new();
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Skip string literals
        if bytes[pos] == b'\'' || bytes[pos] == b'"' {
            let quote = bytes[pos];
            pos += 1;
            while pos < len {
                if bytes[pos] == b'\\' {
                    pos += 1;
                } else if bytes[pos] == quote {
                    break;
                }
                pos += 1;
            }
            pos += 1;
            continue;
        }

        // Skip line comments
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            while pos < len && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }

        // Skip block comments
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos += 2;
            while pos + 1 < len {
                if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                    pos += 2;
                    break;
                }
                pos += 1;
            }
            continue;
        }

        // Look for `try`
        if pos + 3 <= len && &body[pos..pos + 3] == "try" {
            let before_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
            let after_ok = pos + 3 >= len
                || (!bytes[pos + 3].is_ascii_alphanumeric() && bytes[pos + 3] != b'_');
            if before_ok && after_ok {
                // Find the opening brace of the try block
                let after_try = &body[pos + 3..];
                if let Some(brace_offset) = after_try.find('{') {
                    let try_body_start = pos + 3 + brace_offset;
                    // Find the matching closing brace
                    if let Some(try_body_end) =
                        crate::util::find_matching_forward(body, try_body_start, b'{', b'}')
                    {
                        // Now look for `catch` after the try block's `}`
                        let mut catch_search = try_body_end + 1;
                        while catch_search < len {
                            let remaining = body[catch_search..].trim_start();
                            let remaining_start = len - remaining.len();
                            if let Some(after_catch) = remaining.strip_prefix("catch") {
                                // Ensure `catch` is a whole word
                                if after_catch
                                    .bytes()
                                    .next()
                                    .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
                                {
                                    break;
                                }
                                let catch_keyword_len = "catch".len();
                                // Extract caught types from `catch (Type1 | Type2 $var)`
                                if let Some(open_p) = after_catch.find('(') {
                                    let paren_content_start = catch_keyword_len + open_p + 1;
                                    if let Some(close_p) =
                                        remaining[paren_content_start..].find(')')
                                    {
                                        let paren_content = &remaining
                                            [paren_content_start..paren_content_start + close_p];
                                        let (type_names, var_name) =
                                            parse_catch_types(paren_content);

                                        // Skip past the catch block body
                                        let after_close_paren =
                                            remaining_start + paren_content_start + close_p + 1;
                                        if let Some(cb) = body[after_close_paren..].find('{') {
                                            let cb_start = after_close_paren + cb;
                                            if let Some(cb_end) = crate::util::find_matching_forward(
                                                body, cb_start, b'{', b'}',
                                            ) {
                                                if !type_names.is_empty() {
                                                    results.push(CatchInfo {
                                                        type_names,
                                                        var_name,
                                                        try_start: try_body_start,
                                                        try_end: try_body_end,
                                                        catch_body_start: cb_start,
                                                        catch_body_end: cb_end,
                                                    });
                                                }
                                                catch_search = cb_end + 1;
                                                continue;
                                            }
                                        }
                                    }
                                }
                                break;
                            } else if remaining.starts_with("finally") {
                                // Skip finally block, no more catches
                                break;
                            } else {
                                break;
                            }
                        }

                        // Continue scanning INSIDE the try body so that
                        // nested try-catch blocks are discovered.  We
                        // advance past the opening `{` to avoid
                        // re-matching the outer `try` keyword.
                        pos = try_body_start + 1;
                        continue;
                    }
                }
            }
        }

        pos += 1;
    }

    results
}

/// Parse the content inside `catch ( … )` into individual type names and
/// the optional variable name.
///
/// Handles multi-catch: `ExceptionA | ExceptionB $e`
/// → `(["ExceptionA", "ExceptionB"], Some("$e"))`.
fn parse_catch_types(paren_content: &str) -> (Vec<PhpType>, Option<String>) {
    let mut types = Vec::new();

    // Extract the variable name (starts with `$`)
    let var_name = if let Some(dollar) = paren_content.rfind('$') {
        let rest = &paren_content[dollar..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
            .unwrap_or(rest.len());
        let name = rest[..end].trim();
        if name.len() > 1 {
            Some(name.to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Remove the variable name to isolate the type list
    let without_var = if let Some(dollar) = paren_content.rfind('$') {
        &paren_content[..dollar]
    } else {
        paren_content
    };

    // Parse the (possibly multi-catch) type list through the shared type
    // parser and flatten any union into its individual members. Downstream
    // consumers resolve each type via `base_name()`/`normalize_throw_type`,
    // both of which strip any leading `\`, so no normalization is needed here.
    let type_list = without_var.trim();
    if !type_list.is_empty() {
        match PhpType::parse(type_list) {
            PhpType::Union(members) => types.extend(members),
            single => types.push(single),
        }
    }

    (types, var_name)
}

/// Determine which exception types in a function body are **not** caught
/// by an enclosing `try/catch` block.
///
/// Detects six patterns (same-file only):
/// 1. `throw new ExceptionType(…)` (direct instantiation)
/// 2. `throw $this->method()` / `throw self::method()` / `throw static::method()`
///    (the method's return type is the thrown exception type)
/// 3. `throw functionName()` (bare function call, return type is thrown)
/// 4. `$this->method()` / `self::method()` calls where the called method's
///    docblock declares `@throws ExceptionType` (propagated throws, same file)
/// 5. Inline `/** @throws ExceptionType */` annotations in the function body
/// 6. `throw $variable` (resolved through enclosing catch clause variable)
///
/// Returns a deduplicated list of short exception type names.
///
/// This variant does **not** perform cross-file resolution.
/// Use [`find_uncaught_throw_types_with_context`] with a [`ThrowsContext`]
/// to enable it.
pub(crate) fn find_uncaught_throw_types(content: &str, position: Position) -> Vec<PhpType> {
    find_uncaught_throw_types_with_context(content, position, None)
}

/// Like [`find_uncaught_throw_types`] but with an optional [`ThrowsContext`]
/// for cross-file throws propagation.
///
/// When a context is provided, **every** call in the function body is
/// inspected for cross-file `@throws` tags:
///
/// - `$variable->method()` — the variable's type is resolved from the
///   function's parameter list, the class is loaded, and the method's
///   `@throws` tags are propagated.
/// - `ClassName::staticMethod()` — the class is loaded directly and the
///   method's `@throws` tags are propagated.
/// - `functionName()` — the function is loaded and its `@throws` tags
///   are propagated.
/// - `new ClassName(…)` — the class is loaded and the constructor's
///   `@throws` tags are propagated.
pub(crate) fn find_uncaught_throw_types_with_context(
    content: &str,
    position: Position,
    ctx: Option<&ThrowsContext<'_>>,
) -> Vec<PhpType> {
    let body = match extract_function_body(content, position) {
        Some(b) => b,
        None => return Vec::new(),
    };

    let throws = find_throw_statements(&body);
    let throw_expr_types = find_throw_expression_types(&body, content);
    let propagated = find_propagated_throws(&body, content);
    let catches = find_catch_blocks(&body);
    let throw_vars = find_throw_variable_types(&body, &catches);

    // Cross-file propagated throws from all call patterns.
    let cross_file_propagated = if let Some(throws_ctx) = ctx {
        let signature = extract_function_signature(content, position);
        find_cross_file_propagated_throws(&body, &signature, content, throws_ctx)
    } else {
        Vec::new()
    };

    let mut uncaught: Vec<PhpType> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    /// Check whether a throw at `offset` in the function body is caught
    /// by one of the `catches`, given the exception type.
    fn is_caught_by(
        catches: &[CatchInfo],
        offset: usize,
        exc_type: &PhpType,
        use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
        class_loader: OptClassLoader<'_>,
    ) -> bool {
        let exc_name = exc_type.base_name().unwrap_or("");
        // Resolve exception type to FQN via class loader if available.
        let exc_fqn = if let Some(loader) = class_loader {
            loader(exc_name)
                .map(|cls| cls.fqn().to_string())
                .unwrap_or_else(|| exc_name.to_string())
        } else {
            exc_name.to_string()
        };
        catches.iter().any(|c| {
            offset > c.try_start
                && offset < c.try_end
                && c.type_names.iter().any(|ct| {
                    let ct_name = ct.base_name().unwrap_or("");
                    // Resolve the catch type to FQN
                    let resolved_ct = crate::util::resolve_to_fqn(ct_name, use_map, file_namespace);
                    // Also try through the class loader for root-namespace classes.
                    let resolved_ct = if let Some(loader) = class_loader {
                        loader(&resolved_ct)
                            .or_else(|| loader(ct_name))
                            .map(|cls| cls.fqn().to_string())
                            .unwrap_or(resolved_ct)
                    } else {
                        resolved_ct
                    };
                    // Check for Throwable/Exception (catches everything).
                    let resolved_lower = resolved_ct.to_ascii_lowercase();
                    if resolved_lower == "throwable" || resolved_lower == "exception" {
                        return true;
                    }
                    // Exact FQN comparison (case-insensitive)
                    resolved_ct.eq_ignore_ascii_case(&exc_fqn)
                })
        })
    }

    /// Normalize a ThrowInfo's type_name into a PhpType::Named,
    /// resolving short names to FQN when use_map/namespace are available.
    fn normalize_throw_type(
        ty: &PhpType,
        use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
        class_loader: OptClassLoader<'_>,
    ) -> Option<PhpType> {
        let raw = ty.to_string();
        let trimmed = raw.trim_start_matches('\\');
        if trimmed.is_empty() {
            None
        } else {
            let resolved = crate::util::resolve_to_fqn(trimmed, use_map, file_namespace);
            // Try the class loader for a canonical FQN.
            let fqn = if let Some(loader) = class_loader {
                loader(&resolved)
                    .or_else(|| loader(trimmed))
                    .map(|cls| cls.fqn().to_string())
                    .unwrap_or(resolved)
            } else {
                resolved
            };
            Some(PhpType::Named(fqn))
        }
    }

    // Determine name-resolution context: use the ThrowsContext fields
    // when available, fall back to empty defaults.
    let empty_use_map = HashMap::new();
    let empty_ns = None;
    let (use_map, file_namespace) = if let Some(throws_ctx) = ctx {
        (throws_ctx.use_map, throws_ctx.file_namespace)
    } else {
        (&empty_use_map, &empty_ns)
    };

    let class_loader: OptClassLoader<'_> =
        ctx.map(|c| c.class_loader as &dyn Fn(&str) -> Option<Arc<ClassInfo>>);

    // 1. Direct `throw new Type(…)` statements
    for throw in &throws {
        if let Some(exc_type) =
            normalize_throw_type(&throw.type_name, use_map, file_namespace, class_loader)
            && !is_caught_by(
                &catches,
                throw.offset,
                &exc_type,
                use_map,
                file_namespace,
                class_loader,
            )
            && seen.insert(exc_type.to_string())
        {
            uncaught.push(exc_type);
        }
    }

    // 2. `throw $this->method()` -- return type of method is the thrown type
    for te in &throw_expr_types {
        if let Some(exc_type) =
            normalize_throw_type(&te.type_name, use_map, file_namespace, class_loader)
            && !is_caught_by(
                &catches,
                te.offset,
                &exc_type,
                use_map,
                file_namespace,
                class_loader,
            )
            && seen.insert(exc_type.to_string())
        {
            uncaught.push(exc_type);
        }
    }

    // 3. Propagated @throws from called methods (same-file text search)
    for prop in &propagated {
        if let Some(exc_type) =
            normalize_throw_type(&prop.type_name, use_map, file_namespace, class_loader)
            && !is_caught_by(
                &catches,
                prop.offset,
                &exc_type,
                use_map,
                file_namespace,
                class_loader,
            )
            && seen.insert(exc_type.to_string())
        {
            uncaught.push(exc_type);
        }
    }

    // 4. Inline `/** @throws ExceptionType */` annotations in the body
    let inline = find_inline_throws_annotations(&body);
    for info in &inline {
        if let Some(exc_type) =
            normalize_throw_type(&info.type_name, use_map, file_namespace, class_loader)
            && !is_caught_by(
                &catches,
                info.offset,
                &exc_type,
                use_map,
                file_namespace,
                class_loader,
            )
            && seen.insert(exc_type.to_string())
        {
            uncaught.push(exc_type);
        }
    }

    // 5. `throw $variable` — resolved from catch clause variable type
    for tv in &throw_vars {
        if let Some(exc_type) =
            normalize_throw_type(&tv.type_name, use_map, file_namespace, class_loader)
            && !is_caught_by(
                &catches,
                tv.offset,
                &exc_type,
                use_map,
                file_namespace,
                class_loader,
            )
            && seen.insert(exc_type.to_string())
        {
            uncaught.push(exc_type);
        }
    }

    // 6. Cross-file propagated @throws from all call patterns
    for prop in &cross_file_propagated {
        if let Some(exc_type) =
            normalize_throw_type(&prop.type_name, use_map, file_namespace, class_loader)
            && !is_caught_by(
                &catches,
                prop.offset,
                &exc_type,
                use_map,
                file_namespace,
                class_loader,
            )
            && seen.insert(exc_type.to_string())
        {
            uncaught.push(exc_type);
        }
    }

    uncaught
}

/// Extract the function/method signature (the text between `function` and `{`)
/// from the content at the given position.
///
/// Returns the raw signature string, e.g.
/// `"handle(BusinessCentralService $service): void"`.
fn extract_function_signature(content: &str, position: Position) -> String {
    let byte_offset = position_to_byte_offset(content, position);
    let after_cursor = &content[byte_offset.min(content.len())..];

    let after_docblock = if let Some(close_pos) = after_cursor.find("*/") {
        &after_cursor[close_pos + 2..]
    } else {
        after_cursor
    };

    // Find the `function` keyword.
    let lower = after_docblock.to_lowercase();
    let func_pos = match lower.find("function") {
        Some(p) => p,
        None => return String::new(),
    };

    let after_func = &after_docblock[func_pos + 8..]; // skip "function"

    // Everything up to the opening brace is the signature.
    match after_func.find('{') {
        Some(brace) => after_func[..brace].to_string(),
        None => String::new(),
    }
}

/// Parse a function signature to build a map of `$variable_name -> TypeName`.
///
/// Given a signature like `"handle(BusinessCentralService $service, int $count): void"`,
/// returns `[("$service", "BusinessCentralService"), ("$count", "int")]`.
fn parse_param_type_map(signature: &str) -> Vec<(String, PhpType)> {
    let mut result = Vec::new();

    // Extract the text inside the outermost parentheses.
    let open = match signature.find('(') {
        Some(p) => p,
        None => return result,
    };
    let close = match signature.rfind(')') {
        Some(p) => p,
        None => return result,
    };
    if close <= open {
        return result;
    }

    let params_text = &signature[open + 1..close];

    // Split on commas, respecting nested parentheses/generics.
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = params_text.as_bytes();
    let mut segments = Vec::new();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' | b'<' | b'[' | b'{' => depth += 1,
            b')' | b'>' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                segments.push(&params_text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    segments.push(&params_text[start..]);

    for segment in segments {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Each parameter segment looks like:
        //   [?]TypeName [&][$]varName [= default]
        // We need to find the last `$name` token and the type before it.
        // Skip promoted property modifiers (public/protected/private/readonly).
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();

        // Find the variable name (starts with `$`, possibly prefixed with `&` or `...`).
        let var_idx = tokens.iter().position(|t| {
            let t = t.trim_start_matches('&').trim_start_matches("...");
            t.starts_with('$')
        });

        let var_idx = match var_idx {
            Some(i) => i,
            None => continue,
        };

        if var_idx == 0 {
            // No type before the variable name.
            continue;
        }

        let var_name = tokens[var_idx]
            .trim_start_matches('&')
            .trim_start_matches("...");

        // The type is immediately before the variable. Skip modifiers.
        let type_idx = var_idx - 1;
        let type_token = tokens[type_idx];

        // Skip PHP modifiers that aren't types.
        if matches!(
            type_token.to_lowercase().as_str(),
            "public" | "protected" | "private" | "readonly"
        ) {
            continue;
        }

        // Clean the type: strip nullable wrapper and extract base name.
        let parsed = PhpType::parse(type_token);
        let non_null = parsed.non_null_type().unwrap_or_else(|| parsed.clone());
        if let Some(name) = non_null.base_name() {
            result.push((var_name.to_string(), PhpType::Named(name.to_string())));
        } else {
            // Fallback for scalars and other non-class types.
            let cleaned_type = type_token.trim_start_matches('?').trim_start_matches('\\');
            if !cleaned_type.is_empty() {
                result.push((
                    var_name.to_string(),
                    PhpType::Named(cleaned_type.to_string()),
                ));
            }
        }
    }

    result
}

/// Find `@throws` annotations from all call patterns in the function body
/// by resolving types via the class and function loaders.
///
/// Handles:
/// - `$variable->method()` — resolves variable type from function params
/// - `ClassName::staticMethod()` — loads the class directly
/// - `functionName()` — loads the function directly
/// - `new ClassName(…)` — loads the class and checks the constructor
fn find_cross_file_propagated_throws(
    body: &str,
    signature: &str,
    file_content: &str,
    ctx: &ThrowsContext<'_>,
) -> Vec<ThrowInfo> {
    let raw_param_map = parse_param_type_map(signature);
    let class_loader = ctx.class_loader;

    // Resolve short param type names to FQN so that class_loader lookups
    // succeed even when the signature uses unqualified class names.
    let param_map: Vec<(String, PhpType)> = raw_param_map
        .into_iter()
        .map(|(name, ty)| {
            let resolved = if let Some(base) = ty.base_name() {
                PhpType::Named(crate::util::resolve_to_fqn(
                    base,
                    ctx.use_map,
                    ctx.file_namespace,
                ))
            } else {
                ty
            };
            (name, resolved)
        })
        .collect();

    let mut results = Vec::new();
    let mut seen_calls = std::collections::HashSet::new();

    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Skip strings.
        if bytes[pos] == b'\'' || bytes[pos] == b'"' {
            pos = skip_string_forward(bytes, pos);
            continue;
        }
        // Skip line comments.
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            pos = skip_line_comment(bytes, pos);
            continue;
        }
        // Skip block comments.
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos = skip_block_comment(bytes, pos);
            continue;
        }

        // ── Pattern: `new ClassName(…)` ─────────────────────────────
        if pos + 3 < len && &body[pos..pos + 3] == "new" {
            let before_ok =
                pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric() && bytes[pos - 1] != b'_';
            let after_ok = pos + 3 >= len
                || (!bytes[pos + 3].is_ascii_alphanumeric() && bytes[pos + 3] != b'_');
            if before_ok && after_ok {
                let call_start = pos;
                let after_new = body[pos + 3..].trim_start();
                // Extract class name (may be namespaced with `\`).
                let name_end = after_new
                    .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '\\')
                    .unwrap_or(after_new.len());
                let class_name = &after_new[..name_end];
                let after_name = after_new[name_end..].trim_start();
                if !class_name.is_empty() && after_name.starts_with('(') {
                    let clean = class_name.trim_start_matches('\\');
                    // Resolve short class name to FQN before loading.
                    let resolved_class =
                        crate::util::resolve_to_fqn(clean, ctx.use_map, ctx.file_namespace);
                    let call_key = format!("new:{}", resolved_class);
                    if seen_calls.insert(call_key)
                        && let Some(class_info) = class_loader(&resolved_class)
                        && let Some(ctor) = class_info.get_method("__construct")
                    {
                        for exc_type in &ctor.throws {
                            results.push(ThrowInfo {
                                type_name: exc_type.clone(),
                                offset: call_start,
                            });
                        }
                    }
                }
                pos += 3;
                continue;
            }
        }

        // ── Pattern: `$variable->method()` ──────────────────────────
        if bytes[pos] == b'$' {
            let var_start = pos;
            pos += 1;
            // Collect variable name characters.
            while pos < len && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
            }
            let var_name = &body[var_start..pos];

            // Check for `->` immediately after (whitespace-tolerant).
            let rest = &body[pos..];
            let trimmed = rest.trim_start();
            if !trimmed.starts_with("->") {
                continue;
            }
            let arrow_offset = rest.len() - trimmed.len() + 2; // skip "->"
            let after_arrow = &rest[arrow_offset..];
            let after_arrow_trimmed = after_arrow.trim_start();

            // Extract method name.
            let name_end = after_arrow_trimmed
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(after_arrow_trimmed.len());
            let method_name = &after_arrow_trimmed[..name_end];

            if method_name.is_empty() {
                continue;
            }

            // Check that it's followed by `(` (a method call, not a property).
            let after_method = after_arrow_trimmed[name_end..].trim_start();
            if !after_method.starts_with('(') {
                continue;
            }

            // Skip `$this` — those are handled by find_propagated_throws.
            if var_name == "$this" {
                continue;
            }

            // De-duplicate: only process each (variable, method) pair once.
            let call_key = format!("{}::{}", var_name, method_name);
            if !seen_calls.insert(call_key) {
                continue;
            }

            // Look up the variable's type from the parameter map.
            let class_name = match param_map.iter().find(|(name, _)| name == var_name) {
                Some((_, type_name)) => type_name.base_name().unwrap_or(""),
                None => continue,
            };

            // Load the class and find the method's @throws tags.
            collect_method_throws(
                class_loader,
                class_name,
                method_name,
                var_start,
                &mut results,
            );

            continue;
        }

        // ── Pattern: identifier — could be `ClassName::method()` or `functionName()` ──
        if bytes[pos].is_ascii_alphabetic() || bytes[pos] == b'_' || bytes[pos] == b'\\' {
            let ident_start = pos;
            // Collect identifier characters (including namespace separators).
            while pos < len
                && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_' || bytes[pos] == b'\\')
            {
                pos += 1;
            }
            let ident = &body[ident_start..pos];

            let after_ident = body[pos..].trim_start();

            // ── Sub-pattern: `ClassName::method()` ──────────────────
            if let Some(after_colons_raw) = after_ident.strip_prefix("::") {
                let after_colons = after_colons_raw.trim_start();
                let method_end = after_colons
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(after_colons.len());
                let method_name = &after_colons[..method_end];
                let after_method = after_colons[method_end..].trim_start();

                if !method_name.is_empty() && after_method.starts_with('(') {
                    // Skip self::/static::/parent:: — handled by same-file propagation.
                    let ident_lower = ident.to_lowercase();
                    if ident_lower != "self" && ident_lower != "static" && ident_lower != "parent" {
                        let clean_class = ident.trim_start_matches('\\');
                        // Resolve short class name to FQN before loading.
                        let resolved_class = crate::util::resolve_to_fqn(
                            clean_class,
                            ctx.use_map,
                            ctx.file_namespace,
                        );
                        let call_key = format!("{}::{}", resolved_class, method_name);
                        if seen_calls.insert(call_key) {
                            collect_method_throws(
                                class_loader,
                                &resolved_class,
                                method_name,
                                ident_start,
                                &mut results,
                            );
                        }
                    }
                }
                continue;
            }

            // ── Sub-pattern: `functionName()` ───────────────────────
            if after_ident.starts_with('(') {
                // Skip PHP keywords that look like function calls.
                let ident_lower = ident.to_lowercase();
                if !is_php_keyword(&ident_lower) {
                    let clean_name = ident.trim_start_matches('\\');
                    let call_key = format!("fn:{}", clean_name);
                    if seen_calls.insert(call_key) {
                        // First check same-file methods (already handled by
                        // find_propagated_throws for $this->/self::/static::).
                        // For standalone functions, use the function loader.
                        if let Some(func_loader) = ctx.function_loader
                            && let Some(func_info) = func_loader(clean_name)
                        {
                            for exc_type in &func_info.throws {
                                results.push(ThrowInfo {
                                    type_name: exc_type.clone(),
                                    offset: ident_start,
                                });
                            }
                        }
                        // Also check: it might be a same-file function with @throws
                        // in its docblock (text-search fallback).
                        let same_file_throws = find_method_throws_tags(file_content, clean_name);
                        for t in same_file_throws {
                            results.push(ThrowInfo {
                                type_name: t,
                                offset: ident_start,
                            });
                        }
                    }
                }
            }

            continue;
        }

        pos += 1;
    }

    results
}

/// Load a class by name and collect its method's `@throws` into `results`.
fn collect_method_throws(
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    class_name: &str,
    method_name: &str,
    offset: usize,
    results: &mut Vec<ThrowInfo>,
) {
    if let Some(class_info) = class_loader(class_name)
        && let Some(method_info) = class_info.get_method(method_name)
    {
        for exc_type in &method_info.throws {
            results.push(ThrowInfo {
                type_name: exc_type.clone(),
                offset,
            });
        }
    }
}

/// Check whether an identifier is a PHP keyword that should not be
/// treated as a function call (e.g. `if(…)`, `foreach(…)`, `return`).
fn is_php_keyword(ident: &str) -> bool {
    matches!(
        ident,
        "if" | "else"
            | "elseif"
            | "while"
            | "for"
            | "foreach"
            | "switch"
            | "match"
            | "return"
            | "echo"
            | "print"
            | "isset"
            | "unset"
            | "empty"
            | "list"
            | "array"
            | "die"
            | "exit"
            | "eval"
            | "catch"
            | "throw"
            | "yield"
            | "clone"
            | "include"
            | "include_once"
            | "require"
            | "require_once"
            | "new"
            | "self"
            | "static"
            | "parent"
            | "fn"
            | "function"
            | "class"
    )
}

// ─── Import Helpers ─────────────────────────────────────────────────────────

/// Check whether a `use` statement for the given FQN already exists in
/// the file content.
pub(in crate::completion) fn has_use_import(content: &str, fqn: &str) -> bool {
    let target = format!("use {};", fqn);
    let target_with_alias = format!("use {} as", fqn); // alias import
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == target || trimmed.starts_with(&target_with_alias) {
            return true;
        }
        // Handle group imports: `use Foo\{Bar, Baz};`
        // Check if the FQN's namespace prefix is used in a group import
        // that includes the short name.
        if let Some(ns_sep) = fqn.rfind('\\') {
            let ns_prefix = &fqn[..ns_sep];
            let short = &fqn[ns_sep + 1..];
            let group_prefix = format!("use {}\\{{", ns_prefix);
            if trimmed.starts_with(&group_prefix) {
                // Check if short name is in the brace list
                if let Some(brace_start) = trimmed.find('{')
                    && let Some(brace_end) = trimmed.find('}')
                {
                    let names = &trimmed[brace_start + 1..brace_end];
                    if names.split(',').any(|n| n.trim() == short) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "throws_analysis_tests.rs"]
mod tests;
