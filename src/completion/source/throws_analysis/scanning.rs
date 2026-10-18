//! Text scanners for the exceptions a `try` block throws.
//!
//! Catch completion runs while the `catch (…)` clause is still being
//! typed, on code that does not parse yet, so it reads the `try` block's
//! text instead of its syntax tree:
//!   - Find `throw new Type(…)` statements in a block of PHP code
//!   - Find `throw $this->method(…)` / `throw self::method(…)` patterns
//!     and resolve the called method's return type
//!   - Find method calls and collect their `@throws` docblock annotations
//!   - Look up a method's return type from its declaration or docblock
//!   - Look up a method's `@throws` tags from its docblock

use crate::atom::atom;
use crate::parser::preceding_docblock_text;
use crate::php_type::PhpType;
use crate::text_scan::{CodeOffsets, find_matching_forward, is_ident_byte, keyword_starts_at};

// ─── Core Scanning Primitives ───────────────────────────────────────────────

/// Find all `throw new Type(…)` statements in the given PHP source text,
/// as the type names written.
pub(crate) fn find_throw_statements(body: &str) -> Vec<PhpType> {
    let mut results = Vec::new();
    let bytes = body.as_bytes();

    for pos in CodeOffsets::new(bytes) {
        if let Some(after_new) = throw_new_at(body, pos) {
            let type_end = after_new
                .find(|c: char| !c.is_alphanumeric() && c != '\\' && c != '_')
                .unwrap_or(after_new.len());
            let type_name = &after_new[..type_end];
            if !type_name.is_empty() {
                results.push(PhpType::named(atom(type_name)));
            }
        }
    }

    results
}

/// The text after the `throw` keyword at `pos` in `body`, whitespace
/// trimmed, or `None` when no `throw` keyword starts there.
pub(super) fn after_throw_at(body: &str, pos: usize) -> Option<&str> {
    keyword_starts_at(body.as_bytes(), pos, b"throw").then(|| body[pos + 5..].trim_start())
}

/// The text after `throw new` at `pos` in `body`, whitespace trimmed, or
/// `None` when no `throw new` starts there.
fn throw_new_at(body: &str, pos: usize) -> Option<&str> {
    let after_throw = after_throw_at(body, pos)?;
    keyword_starts_at(after_throw.as_bytes(), 0, b"new").then(|| after_throw[3..].trim_start())
}

/// Find `throw $this->method(…)` / `throw self::method(…)` /
/// `throw static::method(…)` patterns and resolve the called method's
/// return type from its declaration or docblock in the same file.
///
/// Returns the type each resolved throw-expression throws.
pub(crate) fn find_throw_expression_types(body: &str, file_content: &str) -> Vec<PhpType> {
    let mut results = Vec::new();
    let method_patterns: &[&str] = &["$this->", "self::", "static::"];

    for pos in CodeOffsets::new(body.as_bytes()) {
        let Some(after_throw) = after_throw_at(body, pos) else {
            continue;
        };
        // Skip `throw new` (handled by find_throw_statements)
        if keyword_starts_at(after_throw.as_bytes(), 0, b"new") {
            continue;
        }
        let mut matched = false;
        // Try method-call patterns first: $this->m(), self::m(), static::m()
        for pat in method_patterns {
            if let Some(rest) = after_throw.strip_prefix(pat) {
                let name_end = rest
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                let method_name = &rest[..name_end];
                if !method_name.is_empty()
                    && let Some(ret_type) = find_method_return_type(file_content, method_name)
                {
                    results.push(ret_type);
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
                results.push(ret_type);
            }
        }
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
/// Returns the type of each propagated throw.
pub(crate) fn find_propagated_throws(body: &str, file_content: &str) -> Vec<PhpType> {
    let mut results = Vec::new();
    let mut seen_methods = std::collections::HashSet::new();
    let patterns: &[&str] = &["$this->", "self::", "static::"];

    let bytes = body.as_bytes();

    for pos in CodeOffsets::new(bytes) {
        for pat in patterns {
            if bytes[pos..].starts_with(pat.as_bytes()) {
                let before_ok = *pat == "$this->" || pos == 0 || !is_ident_byte(bytes[pos - 1]);
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
                        results.push(t);
                    }
                }
                break;
            }
        }
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
pub(crate) fn find_inline_throws_annotations(body: &str) -> Vec<PhpType> {
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
                    use crate::docblock::TagKind;
                    for tag in info.tags_by_kind(TagKind::Throws) {
                        let rest = tag.description.trim();
                        if let Some(type_name) = rest.split_whitespace().next() {
                            let clean = type_name
                                .trim_start_matches('\\')
                                .trim_end_matches('*')
                                .trim_end_matches('/');
                            if !clean.is_empty() && !clean.starts_with('$') {
                                results.push(PhpType::named(atom(clean)));
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
    let (decl_start, after_name) = find_method_declaration(file_content, method_name)?;

    // Check the native return type: find matching `)` then `: Type`
    let after = &file_content[after_name..];
    if let Some(paren_start) = after.find('(')
        && let Some(close_offset) = find_matching_forward(after, paren_start, b'(', b')')
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
                    return Some(PhpType::named(atom(name)));
                }
            }
        }
    }

    let docblock = preceding_docblock_text(file_content, decl_start)?;
    let info = crate::docblock::parser::parse_docblock_for_tags(docblock)?;
    let tag = info.first_tag_by_kind(crate::docblock::TagKind::Return)?;
    let parsed = PhpType::parse(tag.description.split_whitespace().next()?);
    let non_null = parsed.non_null_type().unwrap_or_else(|| parsed.clone());
    let name = non_null.base_name()?;
    (!parsed.is_void() && !parsed.is_mixed() && !parsed.is_self_like())
        .then(|| PhpType::named(atom(name)))
}

/// Find `@throws` tags in a method's docblock by scanning the file
/// content for the method declaration.
///
/// Skips visibility and modifier keywords between the docblock and the
/// `function` keyword.
///
/// Returns the short type names declared in `@throws` tags.
pub(crate) fn find_method_throws_tags(file_content: &str, method_name: &str) -> Vec<PhpType> {
    let Some(info) = find_method_declaration(file_content, method_name)
        .and_then(|(decl_start, _)| preceding_docblock_text(file_content, decl_start))
        .and_then(crate::docblock::parser::parse_docblock_for_tags)
    else {
        return Vec::new();
    };
    info.tags_by_kind(crate::docblock::TagKind::Throws)
        .filter_map(|tag| {
            let clean = tag
                .description
                .split_whitespace()
                .next()?
                .trim_end_matches('/')
                .trim_end_matches('*')
                .trim_start_matches('\\');
            (!clean.is_empty()).then(|| PhpType::named(atom(clean)))
        })
        .collect()
}

/// Where the first `function <method_name>` declaration in `file_content`
/// starts, and the offset just past the name.  Only code counts, so a
/// comment or a string naming the method is not taken for it, and the name
/// must end there: `function foo` does not match `function foobar`.
fn find_method_declaration(file_content: &str, method_name: &str) -> Option<(usize, usize)> {
    let bytes = file_content.as_bytes();
    CodeOffsets::new(bytes).find_map(|start| {
        if !keyword_starts_at(bytes, start, b"function") {
            return None;
        }
        // A method that returns by reference is declared `function &name`.
        let after_keyword = file_content[start + "function".len()..].trim_start();
        let name = after_keyword
            .strip_prefix('&')
            .map_or(after_keyword, str::trim_start);
        let name_start = file_content.len() - name.len();
        let name_end = name_start + method_name.len();
        let declares = name
            .get(..method_name.len())
            .is_some_and(|written| written.eq_ignore_ascii_case(method_name))
            && !bytes.get(name_end).is_some_and(|&b| is_ident_byte(b));
        declares.then_some((start, name_end))
    })
}
