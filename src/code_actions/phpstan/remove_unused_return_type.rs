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

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::fix_return_type::{
    find_close_paren_before_brace, find_open_brace_from_declaration, find_return_type_edit,
    native_return_type, paren_function_docblock,
};
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::php_type::{PhpType, TypeKind};

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

    match full_type.kind() {
        TypeKind::Union(members) => {
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
        TypeKind::Intersection(members) => {
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
        TypeKind::Nullable(inner) => {
            // `?T` is equivalent to `T|null`.
            if unused_parsed.equivalent(&PhpType::null()) {
                // Remove the null → just `T`.
                Some((**inner).clone().into())
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
/// constructs a `TypeKind::Union` (for `"|"`) or `TypeKind::Intersection`
/// (for `"&"`).
fn format_type_list(types: &[&PhpType], sep: &str) -> PhpType {
    let cloned: Vec<PhpType> = types.iter().map(|t| (*t).clone()).collect();
    if cloned.len() == 1 {
        return cloned.into_iter().next().unwrap();
    }
    if sep == "&" {
        PhpType::intersection(cloned)
    } else {
        PhpType::union(cloned)
    }
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
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        let lines: Vec<&str> = content.lines().collect();

        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
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
            let native_type = native_return_type(&lines, paren_line, paren_col, brace_line);
            let has_native_match = native_type
                .as_ref()
                .is_some_and(|t| remove_type_from_union(t, unused_type).is_some());

            // Check the docblock @return tag.
            let doc_return_type = paren_function_docblock(content, paren_line, paren_col)
                .and_then(|docblock| docblock.return_type());
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
        if let Some(native_type) = native_return_type(&lines, paren_line, paren_col, brace_line)
            && let Some(new_type) = remove_type_from_union(&native_type, unused_type)
        {
            // Convert the new type to a valid native hint.
            let native_hint = new_type
                .to_native_hint()
                .unwrap_or_else(|| new_type.to_string());

            if let Some(edit) =
                find_return_type_edit(&lines, paren_line, paren_col, brace_line, &native_hint)
            {
                edits.push(edit);
            }
        }

        // ── Update @return docblock tag ─────────────────────────────
        if let Some(docblock) = paren_function_docblock(content, paren_line, paren_col)
            && let Some(doc_type) = docblock.return_type()
            && let Some(new_type) = remove_type_from_union(&doc_type, unused_type)
            && let Some(edit) = docblock.replace_return_type_edit(content, &new_type.to_string())
        {
            edits.push(edit);
        }

        if edits.is_empty() {
            return None;
        }

        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
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
    if let Some(native_type) = native_return_type(&lines, paren_line, paren_col, brace_line)
        && remove_type_from_union(&native_type, unused_type).is_some()
    {
        // The unused type is still present → not stale.
        return false;
    }

    // Check docblock @return tag.
    if let Some(doc_type) = paren_function_docblock(content, paren_line, paren_col)
        .and_then(|docblock| docblock.return_type())
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

    // ── Reading and editing the return types ───────────────────────

    /// The `(paren_line, paren_col, brace_line)` of the function declared
    /// on `decl_line`, the way the action navigates to them.
    fn signature_at(lines: &[&str], decl_line: usize) -> (usize, usize, usize) {
        let brace_line = find_open_brace_from_declaration(lines, decl_line).unwrap();
        let (paren_line, paren_col) = find_close_paren_before_brace(lines, brace_line).unwrap();
        (paren_line, paren_col, brace_line)
    }

    /// The `@return` type of the function declared on `decl_line`.
    fn return_tag_type(content: &str, decl_line: usize) -> Option<PhpType> {
        let lines: Vec<&str> = content.lines().collect();
        let (paren_line, paren_col, _) = signature_at(&lines, decl_line);
        paren_function_docblock(content, paren_line, paren_col)?.return_type()
    }

    #[test]
    fn extracts_native_return_type_simple() {
        let content = "<?php\nfunction foo(): string|null {\n    return 'hello';\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let (paren_line, paren_col, brace_line) = signature_at(&lines, 1);
        let result = native_return_type(&lines, paren_line, paren_col, brace_line);
        assert_eq!(result, Some(PhpType::parse("string|null")));
    }

    #[test]
    fn extracts_native_return_type_nullable() {
        let content = "<?php\nfunction foo(): ?string {\n    return 'hello';\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let (paren_line, paren_col, brace_line) = signature_at(&lines, 1);
        let result = native_return_type(&lines, paren_line, paren_col, brace_line);
        assert_eq!(result, Some(PhpType::parse("?string")));
    }

    #[test]
    fn extracts_return_tag_type_simple() {
        let content =
            "<?php\n/**\n * @return string|null The value\n */\nfunction foo(): string|null {\n}\n";
        assert_eq!(
            return_tag_type(content, 4),
            Some(PhpType::parse("string|null"))
        );
    }

    #[test]
    fn extracts_return_tag_type_no_description() {
        let content = "<?php\n/**\n * @return int|bool\n */\nfunction foo(): int|bool {\n}\n";
        assert_eq!(
            return_tag_type(content, 4),
            Some(PhpType::parse("int|bool"))
        );
    }

    #[test]
    fn extracts_generic_return_tag_type() {
        let content =
            "<?php\n/**\n * @return array<int, string>|null\n */\nfunction foo(): ?array {\n}\n";
        assert_eq!(
            return_tag_type(content, 4),
            Some(PhpType::parse("array<int, string>|null"))
        );
    }

    #[test]
    fn no_return_tag_returns_none() {
        let content = "<?php\n/**\n * Does something.\n */\nfunction foo(): int|bool {\n}\n";
        assert_eq!(return_tag_type(content, 4), None);
    }

    #[test]
    fn replaces_return_tag_type() {
        let content =
            "<?php\n/**\n * @return string|null The value\n */\nfunction foo(): string|null {\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let (paren_line, paren_col, _) = signature_at(&lines, 4);
        let edit = paren_function_docblock(content, paren_line, paren_col)
            .unwrap()
            .replace_return_type_edit(content, "string")
            .unwrap();
        assert_eq!(
            crate::text_position::apply_text_edits(content, &[edit]),
            "<?php\n/**\n * @return string The value\n */\nfunction foo(): string|null {\n}\n"
        );
    }

    #[test]
    fn replaces_native_return_type() {
        let content = "<?php\nfunction foo(): string|null {\n    return 'hello';\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let (paren_line, paren_col, brace_line) = signature_at(&lines, 1);
        let edit =
            find_return_type_edit(&lines, paren_line, paren_col, brace_line, "string").unwrap();
        assert_eq!(edit.new_text, ": string");
    }
}
