//! Catch-block scanning and `throw $variable` resolution.
//!
//! Finds `try { … } catch (…) { … }` blocks and their caught exception
//! types, and resolves `throw $variable` statements to the type caught
//! by the enclosing `catch` clause.

use crate::php_type::PhpType;

use super::scanning::ThrowInfo;

/// Information about a `catch (Type $var)` clause in a function body.
#[derive(Debug)]
pub(crate) struct CatchInfo {
    /// The caught exception type names (multi-catch produces multiple).
    pub type_names: Vec<PhpType>,
    /// The variable name from the catch clause (e.g. `"$e"`), if present.
    pub var_name: Option<String>,
    /// Byte offset of the start of the `try` block this catch belongs to.
    pub try_start: usize,
    /// Byte offset of the end of the `try` block (the matching `}`).
    pub try_end: usize,
    /// Byte offset of the opening `{` of the catch block body.
    pub catch_body_start: usize,
    /// Byte offset of the closing `}` of the catch block body.
    pub catch_body_end: usize,
}

/// Find `throw $variable` patterns and resolve the variable's exception
/// type from catch clauses whose body contains the throw.
///
/// When `throw $e` appears inside a `catch (SomeException $e) { … }` block,
/// the thrown type is `SomeException`.
pub(crate) fn find_throw_variable_types(body: &str, catches: &[CatchInfo]) -> Vec<ThrowInfo> {
    let mut results = Vec::new();
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
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
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            while pos < len && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
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

/// Find all `try { … } catch (…)` blocks and their caught types.
pub(crate) fn find_catch_blocks(body: &str) -> Vec<CatchInfo> {
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
                        crate::text_scan::find_matching_forward(body, try_body_start, b'{', b'}')
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
                                            if let Some(cb_end) =
                                                crate::text_scan::find_matching_forward(
                                                    body, cb_start, b'{', b'}',
                                                )
                                            {
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
pub(crate) fn parse_catch_types(paren_content: &str) -> (Vec<PhpType>, Option<String>) {
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
