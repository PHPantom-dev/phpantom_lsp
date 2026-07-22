//! "Remove unused return type" code action for PHPStan `return.unusedType`.
//!
//! When PHPStan reports that a union or intersection member in the return
//! type is never actually returned, this code action removes the unused
//! member from both the native return type declaration and the `@return`
//! docblock tag.
//!
//! **Messages:**
//! - `Method Foo::bar() never returns {type} so it can be removed from the return type.`
//! - `Function foo() never returns {type} so it can be removed from the return type.`
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_remove_unused_return_type_actions`) validates that the
//! action is applicable and emits a lightweight `CodeAction` with a `data`
//! payload but no `edit`.  Phase 2 (`resolve_remove_unused_return_type`)
//! recomputes the workspace edit on demand when the user picks the action.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::add_iterable_type::{
    find_function_docblock, find_function_keyword_line as find_func_keyword_line,
};
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::php_type::PhpType;
use crate::util::ranges_overlap;

// ── PHPStan identifier ──────────────────────────────────────────────────────

/// PHPStan identifier for "return type contains a type that is never returned".
const UNUSED_RETURN_TYPE_ID: &str = "return.unusedType";

/// Action kind string for removing an unused type from the return type.
const ACTION_KIND: &str = "phpstan.removeUnusedReturnType";

// ── Message parsing ─────────────────────────────────────────────────────────

/// Extract the unused type from a `return.unusedType` diagnostic message.
///
/// Message format:
/// `{desc} never returns {type} so it can be removed from the return type.`
///
/// Returns the `{type}` string, or `None` if the message doesn't match.
fn extract_unused_type(message: &str) -> Option<&str> {
    let marker = " never returns ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find(" so it can be removed from the return type.")?;
    let unused = rest[..end].trim();
    if unused.is_empty() {
        return None;
    }
    Some(unused)
}

// ── Type removal logic ──────────────────────────────────────────────────────

/// Remove `unused_type` from a type string that may be a union or
/// intersection.
///
/// Returns `Some(new_type_string)` if the type was found and removed,
/// or `None` if the unused type was not found in `full_type`.
///
/// When removing leaves a single member, the union/intersection wrapper
/// is simplified (e.g. `string|null` minus `null` becomes `string`).
///
/// Also handles `?Type` (nullable shorthand): `?string` with unused
/// `null` becomes `string`, and `?string` with unused `string` becomes
/// `null`.
fn remove_type_from_union(full_type: &PhpType, unused_type: &str) -> Option<PhpType> {
    let unused_parsed = PhpType::parse(unused_type);

    match full_type {
        PhpType::Union(members) => {
            let remaining: Vec<&PhpType> = members
                .iter()
                .filter(|m| !m.equivalent(&unused_parsed))
                .collect();

            if remaining.len() == members.len() {
                // The unused type was not found in the union.
                return None;
            }

            if remaining.is_empty() {
                // All members removed — should not happen in practice.
                return None;
            }

            Some(format_type_list(&remaining, "|"))
        }
        PhpType::Intersection(members) => {
            let remaining: Vec<&PhpType> = members
                .iter()
                .filter(|m| !m.equivalent(&unused_parsed))
                .collect();

            if remaining.len() == members.len() {
                return None;
            }

            if remaining.is_empty() {
                return None;
            }

            Some(format_type_list(&remaining, "&"))
        }
        PhpType::Nullable(inner) => {
            // `?T` is equivalent to `T|null`.
            if unused_parsed.equivalent(&PhpType::null()) {
                // Remove the null → just `T`.
                Some((**inner).clone())
            } else if inner.equivalent(&unused_parsed) {
                // Remove the inner type → just `null`.
                Some(PhpType::null())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Build a `PhpType` from a list of type references.
///
/// When only one member remains, returns it directly.  Otherwise
/// constructs a `PhpType::Union` (for `"|"`) or `PhpType::Intersection`
/// (for `"&"`).
fn format_type_list(types: &[&PhpType], sep: &str) -> PhpType {
    let cloned: Vec<PhpType> = types.iter().map(|t| (*t).clone()).collect();
    if cloned.len() == 1 {
        return cloned.into_iter().next().unwrap();
    }
    if sep == "&" {
        PhpType::Intersection(cloned)
    } else {
        PhpType::Union(cloned)
    }
}

// ── Helpers to find and edit native return types ────────────────────────────

/// Find the closing `)` of the parameter list before the opening `{`.
fn find_close_paren_before_brace(lines: &[&str], brace_line: usize) -> Option<(usize, usize)> {
    let brace_text = lines[brace_line];
    if let Some(brace_pos) = brace_text.rfind('{') {
        let before_brace = &brace_text[..brace_pos];
        if let Some(paren_pos) = before_brace.rfind(')') {
            return Some((brace_line, paren_pos));
        }
    }

    for i in (0..brace_line).rev() {
        if let Some(paren_pos) = lines[i].rfind(')') {
            return Some((i, paren_pos));
        }
    }

    None
}

/// Gather the source text between `)` at `(paren_line, paren_col)` and
/// `{` on `brace_line`.
fn gather_between_paren_and_brace(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
) -> String {
    let mut between = String::new();

    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(brace_line + 1)
        .skip(paren_line)
    {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = if line_idx == brace_line {
            line.find('{').unwrap_or(line.len())
        } else {
            line.len()
        };
        if start_col <= end_col {
            between.push_str(&line[start_col..end_col]);
        }
        if line_idx < brace_line {
            between.push('\n');
        }
    }

    between
}

/// Map an offset within the "between" text back to an absolute
/// (line, col) position in the original source.
fn map_between_offset_to_position(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    offset: usize,
) -> Option<(usize, usize)> {
    let mut remaining = offset;
    for (line_idx, line) in lines.iter().enumerate().skip(paren_line) {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = line.len();
        let span = end_col - start_col;

        if remaining <= span {
            return Some((line_idx, start_col + remaining));
        }
        remaining -= span;

        if remaining == 0 {
            return Some((line_idx + 1, 0));
        }
        remaining -= 1; // for the '\n'
    }
    None
}

/// Find the native return type between `)` and `{` and build a
/// `TextEdit` that replaces it with `new_type`.
///
/// Returns `None` if no return type is found.
fn find_and_replace_native_return_type(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
    new_type: &str,
) -> Option<TextEdit> {
    let between = gather_between_paren_and_brace(lines, paren_line, paren_col, brace_line);

    let colon_pos = between.find(':')?;
    let after_colon = &between[colon_pos + 1..];
    let type_start_offset = after_colon.find(|c: char| !c.is_whitespace()).unwrap_or(0);
    let type_text_start = colon_pos + 1 + type_start_offset;
    let type_text = &between[type_text_start..];

    let type_len = type_text
        .find(|c: char| c.is_whitespace() || c == '{')
        .unwrap_or(type_text.len());

    if type_len == 0 {
        return None;
    }

    let colon_abs = map_between_offset_to_position(lines, paren_line, paren_col, colon_pos)?;
    let type_end_abs =
        map_between_offset_to_position(lines, paren_line, paren_col, type_text_start + type_len)?;

    Some(TextEdit {
        range: Range {
            start: Position::new(colon_abs.0 as u32, colon_abs.1 as u32),
            end: Position::new(type_end_abs.0 as u32, type_end_abs.1 as u32),
        },
        new_text: format!(": {}", new_type),
    })
}

/// Extract the current native return type from between `)` and `{`.
fn extract_native_return_type(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
) -> Option<PhpType> {
    let between = gather_between_paren_and_brace(lines, paren_line, paren_col, brace_line);

    let colon_pos = between.find(':')?;
    let after_colon = &between[colon_pos + 1..];
    let type_start_offset = after_colon.find(|c: char| !c.is_whitespace()).unwrap_or(0);
    let type_text_start = colon_pos + 1 + type_start_offset;
    let type_text = &between[type_text_start..];

    let type_len = type_text
        .find(|c: char| c.is_whitespace() || c == '{')
        .unwrap_or(type_text.len());

    if type_len == 0 {
        return None;
    }

    Some(PhpType::parse(&type_text[..type_len]))
}

/// Find the `@return` tag in a docblock and build a `TextEdit` that
/// replaces its type with `new_type`.
///
/// Returns `None` if no `@return` tag exists.
fn find_and_replace_return_tag_type(
    lines: &[&str],
    doc_start: usize,
    doc_end: usize,
    new_type: &str,
) -> Option<TextEdit> {
    for (i, line_text) in lines.iter().enumerate().take(doc_end + 1).skip(doc_start) {
        if let Some(at_pos) = line_text.find("@return") {
            let after_return = &line_text[at_pos + "@return".len()..];
            let type_and_rest = after_return.trim_start();
            let whitespace_before_type = after_return.len() - type_and_rest.len();
            let type_start_in_line = at_pos + "@return".len() + whitespace_before_type;

            let type_end = type_and_rest
                .find(|c: char| c.is_whitespace())
                .unwrap_or(type_and_rest.len());
            let description = type_and_rest[type_end..].to_string();

            let new_line = format!(
                "{}{}{}",
                &line_text[..type_start_in_line],
                new_type,
                description,
            );

            return Some(TextEdit {
                range: Range {
                    start: Position::new(i as u32, 0),
                    end: Position::new(i as u32, line_text.len() as u32),
                },
                new_text: new_line,
            });
        }
    }
    None
}

/// Extract the current `@return` type from a docblock.
fn extract_return_tag_type(lines: &[&str], doc_start: usize, doc_end: usize) -> Option<PhpType> {
    for line_text in lines.iter().take(doc_end + 1).skip(doc_start) {
        if let Some(at_pos) = line_text.find("@return") {
            let after_return = &line_text[at_pos + "@return".len()..];
            let type_and_rest = after_return.trim_start();
            let type_end = type_and_rest
                .find(|c: char| c.is_whitespace())
                .unwrap_or(type_and_rest.len());
            if type_end > 0 {
                return Some(PhpType::parse(&type_and_rest[..type_end]));
            }
        }
    }
    None
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect code actions for PHPStan `return.unusedType` diagnostics.
    pub(crate) fn collect_remove_unused_return_type_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags: Vec<Diagnostic> = {
            let cache = self.phpstan_tool.last_diags.lock();
            cache.get(uri).cloned().unwrap_or_default()
        };

        let lines: Vec<&str> = content.lines().collect();

        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            if identifier != UNUSED_RETURN_TYPE_ID {
                continue;
            }

            let unused_type = match extract_unused_type(&diag.message) {
                Some(t) => t,
                None => continue,
            };

            let diag_line = diag.range.start.line as usize;

            // The diagnostic is reported on the function/method declaration
            // line.  Find the opening `{` and the return type.
            let brace_line = match find_open_brace_from_declaration(&lines, diag_line) {
                Some(l) => l,
                None => continue,
            };
            let (paren_line, paren_col) = match find_close_paren_before_brace(&lines, brace_line) {
                Some(p) => p,
                None => continue,
            };

            // Check that the native return type contains the unused type.
            let native_type = extract_native_return_type(&lines, paren_line, paren_col, brace_line);
            let has_native_match = native_type
                .as_ref()
                .is_some_and(|t| remove_type_from_union(t, unused_type).is_some());

            // Check the docblock @return tag.
            let func_line = find_func_keyword_line(&lines, paren_line).unwrap_or(diag_line);
            let docblock_info = find_function_docblock(&lines, func_line);
            let doc_return_type = if docblock_info.has_docblock && docblock_info.has_return_tag {
                extract_return_tag_type(
                    &lines,
                    docblock_info.doc_start_line,
                    docblock_info.doc_end_line,
                )
            } else {
                None
            };
            let has_doc_match = doc_return_type
                .as_ref()
                .is_some_and(|t| remove_type_from_union(t, unused_type).is_some());

            // Only offer the action if we can actually remove the type
            // from at least one location.
            if !has_native_match && !has_doc_match {
                continue;
            }

            let extra = serde_json::json!({
                "diagnostic_line": diag_line,
                "unused_type": unused_type,
            });

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Remove '{}' from return type", unused_type),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(make_code_action_data(
                    ACTION_KIND,
                    uri,
                    &params.range,
                    extra,
                )),
            }));
        }
    }

    /// Resolve a "Remove unused return type" code action by computing the
    /// full workspace edit.
    pub(crate) fn resolve_remove_unused_return_type(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let extra = &data.extra;
        let diag_line = extra.get("diagnostic_line")?.as_u64()? as usize;
        let unused_type = extra.get("unused_type")?.as_str()?;

        let doc_uri: Url = data.uri.parse().ok()?;
        let lines: Vec<&str> = content.lines().collect();

        if diag_line >= lines.len() {
            return None;
        }

        let brace_line = find_open_brace_from_declaration(&lines, diag_line)?;
        let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

        let mut edits = Vec::new();

        // ── Update native return type ───────────────────────────────
        if let Some(native_type) =
            extract_native_return_type(&lines, paren_line, paren_col, brace_line)
            && let Some(new_type) = remove_type_from_union(&native_type, unused_type)
        {
            // Convert the new type to a valid native hint.
            let native_hint = new_type
                .to_native_hint()
                .unwrap_or_else(|| new_type.to_string());

            if let Some(edit) = find_and_replace_native_return_type(
                &lines,
                paren_line,
                paren_col,
                brace_line,
                &native_hint,
            ) {
                edits.push(edit);
            }
        }

        // ── Update @return docblock tag ─────────────────────────────
        let func_line = find_func_keyword_line(&lines, paren_line).unwrap_or(diag_line);
        let docblock_info = find_function_docblock(&lines, func_line);

        if docblock_info.has_docblock
            && docblock_info.has_return_tag
            && let Some(doc_type) = extract_return_tag_type(
                &lines,
                docblock_info.doc_start_line,
                docblock_info.doc_end_line,
            )
            && let Some(new_type) = remove_type_from_union(&doc_type, unused_type)
            && let Some(edit) = find_and_replace_return_tag_type(
                &lines,
                docblock_info.doc_start_line,
                docblock_info.doc_end_line,
                &new_type.to_string(),
            )
        {
            edits.push(edit);
        }

        if edits.is_empty() {
            return None;
        }

        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);
        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

/// Search forward from a declaration line to find the opening `{`.
fn find_open_brace_from_declaration(lines: &[&str], decl_line: usize) -> Option<usize> {
    let end = (decl_line + 6).min(lines.len());
    (decl_line..end).find(|&i| lines[i].contains('{'))
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a `return.unusedType` diagnostic is stale.
///
/// The diagnostic is stale when the return type no longer contains
/// the unused type as a union or intersection member.
pub(crate) fn is_remove_unused_return_type_stale(
    content: &str,
    diag_line: usize,
    message: &str,
) -> bool {
    let unused_type = match extract_unused_type(message) {
        Some(t) => t,
        None => return true,
    };

    let lines: Vec<&str> = content.lines().collect();

    if diag_line >= lines.len() {
        return true;
    }

    // Find the function's return type area.
    let brace_line = match find_open_brace_from_declaration(&lines, diag_line) {
        Some(l) => l,
        None => return false,
    };
    let (paren_line, paren_col) = match find_close_paren_before_brace(&lines, brace_line) {
        Some(p) => p,
        None => return false,
    };

    // Check native return type.
    if let Some(native_type) = extract_native_return_type(&lines, paren_line, paren_col, brace_line)
        && remove_type_from_union(&native_type, unused_type).is_some()
    {
        // The unused type is still present → not stale.
        return false;
    }

    // Check docblock @return tag.
    let func_line = find_func_keyword_line(&lines, paren_line).unwrap_or(diag_line);
    let docblock_info = find_function_docblock(&lines, func_line);
    if docblock_info.has_docblock
        && docblock_info.has_return_tag
        && let Some(doc_type) = extract_return_tag_type(
            &lines,
            docblock_info.doc_start_line,
            docblock_info.doc_end_line,
        )
        && remove_type_from_union(&doc_type, unused_type).is_some()
    {
        return false;
    }

    // The unused type is no longer present in any return type → stale.
    true
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_unused_type ────────────────────────────────────────

    #[test]
    fn extracts_type_from_method_message() {
        let msg = "Method Foo::bar() never returns null so it can be removed from the return type.";
        assert_eq!(extract_unused_type(msg), Some("null"));
    }

    #[test]
    fn extracts_type_from_function_message() {
        let msg = "Function foo() never returns string so it can be removed from the return type.";
        assert_eq!(extract_unused_type(msg), Some("string"));
    }

    #[test]
    fn extracts_class_type() {
        let msg = "Method Foo::bar() never returns NotFoundException so it can be removed from the return type.";
        assert_eq!(extract_unused_type(msg), Some("NotFoundException"));
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Call to function assert() with true will always evaluate to true.";
        assert_eq!(extract_unused_type(msg), None);
    }

    // ── remove_type_from_union ─────────────────────────────────────

    #[test]
    fn removes_null_from_string_null() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("string|null"), "null"),
            Some(PhpType::parse("string"))
        );
    }

    #[test]
    fn removes_string_from_string_null() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("string|null"), "string"),
            Some(PhpType::parse("null"))
        );
    }

    #[test]
    fn removes_from_three_member_union() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("string|int|null"), "null"),
            Some(PhpType::parse("string|int"))
        );
    }

    #[test]
    fn removes_middle_member() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("string|int|bool"), "int"),
            Some(PhpType::parse("string|bool"))
        );
    }

    #[test]
    fn removes_from_intersection() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("Foo&Bar"), "Bar"),
            Some(PhpType::parse("Foo"))
        );
    }

    #[test]
    fn removes_null_from_nullable() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("?string"), "null"),
            Some(PhpType::parse("string"))
        );
    }

    #[test]
    fn removes_inner_from_nullable() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("?string"), "string"),
            Some(PhpType::parse("null"))
        );
    }

    #[test]
    fn returns_none_when_not_found() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("string|int"), "bool"),
            None
        );
    }

    #[test]
    fn returns_none_for_single_type() {
        assert_eq!(
            remove_type_from_union(&PhpType::parse("string"), "string"),
            None
        );
    }

    // ── stale detection ────────────────────────────────────────────

    #[test]
    fn stale_when_type_removed() {
        let content = "<?php\nfunction foo(): string {\n    return 'hello';\n}\n";
        let msg = "Function foo() never returns null so it can be removed from the return type.";
        assert!(is_remove_unused_return_type_stale(content, 1, msg));
    }

    #[test]
    fn not_stale_when_type_still_present() {
        let content = "<?php\nfunction foo(): string|null {\n    return 'hello';\n}\n";
        let msg = "Function foo() never returns null so it can be removed from the return type.";
        assert!(!is_remove_unused_return_type_stale(content, 1, msg));
    }

    #[test]
    fn stale_when_line_gone() {
        let content = "<?php\n";
        let msg = "Function foo() never returns null so it can be removed from the return type.";
        assert!(is_remove_unused_return_type_stale(content, 5, msg));
    }

    // ── native return type extraction ──────────────────────────────

    #[test]
    fn extracts_native_return_type_simple() {
        let content = "<?php\nfunction foo(): string|null {\n    return 'hello';\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let brace_line = find_open_brace_from_declaration(&lines, 1).unwrap();
        let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line).unwrap();
        let result = extract_native_return_type(&lines, paren_line, paren_col, brace_line);
        assert_eq!(result, Some(PhpType::parse("string|null")));
    }

    #[test]
    fn extracts_native_return_type_nullable() {
        let content = "<?php\nfunction foo(): ?string {\n    return 'hello';\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let brace_line = find_open_brace_from_declaration(&lines, 1).unwrap();
        let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line).unwrap();
        let result = extract_native_return_type(&lines, paren_line, paren_col, brace_line);
        assert_eq!(result, Some(PhpType::parse("?string")));
    }

    // ── @return tag extraction ─────────────────────────────────────

    #[test]
    fn extracts_return_tag_type_simple() {
        let lines = vec![
            "<?php",
            "/**",
            " * @return string|null The value",
            " */",
            "function foo(): string|null {",
        ];
        assert_eq!(
            extract_return_tag_type(&lines, 1, 3),
            Some(PhpType::parse("string|null"))
        );
    }

    #[test]
    fn extracts_return_tag_type_no_description() {
        let lines = vec!["<?php", "/**", " * @return int|bool", " */"];
        assert_eq!(
            extract_return_tag_type(&lines, 1, 3),
            Some(PhpType::parse("int|bool"))
        );
    }

    #[test]
    fn no_return_tag_returns_none() {
        let lines = vec!["<?php", "/**", " * Does something.", " */"];
        assert_eq!(extract_return_tag_type(&lines, 1, 3), None);
    }

    // ── find_and_replace_return_tag_type ────────────────────────────

    #[test]
    fn replaces_return_tag_type() {
        let lines = vec!["<?php", "/**", " * @return string|null The value", " */"];
        let edit = find_and_replace_return_tag_type(&lines, 1, 3, "string").unwrap();
        assert_eq!(edit.range.start.line, 2);
        assert!(
            edit.new_text.contains("@return string"),
            "should have new type: {}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("The value"),
            "should preserve description: {}",
            edit.new_text
        );
    }

    // ── find_and_replace_native_return_type ─────────────────────────

    #[test]
    fn replaces_native_return_type() {
        let content = "<?php\nfunction foo(): string|null {\n    return 'hello';\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let brace_line = find_open_brace_from_declaration(&lines, 1).unwrap();
        let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line).unwrap();
        let edit = find_and_replace_native_return_type(
            &lines, paren_line, paren_col, brace_line, "string",
        )
        .unwrap();
        assert_eq!(edit.new_text, ": string");
    }
}
