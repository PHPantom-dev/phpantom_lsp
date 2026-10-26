//! "Fix PHPDoc type" code actions for PHPStan `return.phpDocType`,
//! `parameter.phpDocType`, and `property.phpDocType`.
//!
//! When PHPStan reports that a `@return`, `@param`, or `@var` tag has a
//! type that is incompatible with (or not a subtype of) the native type
//! hint, these code actions offer two quickfixes:
//!
//! 1. **Update the tag type** to match the native type.
//! 2. **Remove the tag entirely** (marked as `is_preferred` since the
//!    native type is authoritative).
//!
//! After applying either fix the triggering diagnostic is eagerly
//! removed from the PHPStan cache so the user gets instant visual
//! feedback without waiting for the next PHPStan run.
//!
//! **Trigger:** A PHPStan diagnostic with one of the above identifiers
//! overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_fix_phpdoc_type_actions`) validates that the action
//! is applicable and emits lightweight `CodeAction` objects with a
//! `data` payload but no `edit`.  Phase 2 (`resolve_fix_phpdoc_type`)
//! recomputes the workspace edit on demand when the user picks the
//! action.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{
    CodeActionData, DocblockAbove, deferred_quickfix, find_docblock_above_line,
};
use crate::docblock::strip_docblock_line_delimiters;

// ── PHPStan identifiers ─────────────────────────────────────────────────────

/// `@return` type incompatible with native return type.
const RETURN_PHPDOC_TYPE_ID: &str = "return.phpDocType";

/// `@param` type incompatible with native parameter type.
const PARAM_PHPDOC_TYPE_ID: &str = "parameter.phpDocType";

/// `@var` (property) type incompatible with native property type.
const PROPERTY_PHPDOC_TYPE_ID: &str = "property.phpDocType";

/// All identifiers handled by this module.
const ALL_IDS: &[&str] = &[
    RETURN_PHPDOC_TYPE_ID,
    PARAM_PHPDOC_TYPE_ID,
    PROPERTY_PHPDOC_TYPE_ID,
];

// ── Parsed mismatch ─────────────────────────────────────────────────────────

/// Information extracted from a PHPDoc type mismatch diagnostic.
#[derive(Debug, Clone)]
struct PhpDocMismatch {
    /// The tag kind (`@return`, `@param`, or `@var`).
    tag: &'static str,
    /// The PHPDoc type that PHPStan considers wrong.
    phpdoc_type: String,
    /// The native type that PHPStan considers authoritative.
    native_type: String,
    /// For `@param`: the parameter name (including `$`).
    /// `None` for `@return` and `@var`.
    param_name: Option<String>,
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Fix PHPDoc type" code actions for PHPStan
    /// `return.phpDocType`, `parameter.phpDocType`, and
    /// `property.phpDocType` diagnostics.
    pub(crate) fn collect_fix_phpdoc_type_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
            };

            if !ALL_IDS.contains(&identifier) {
                continue;
            }

            let mismatch =
                match parse_mismatch(crate::util::message_text(&diag.message), identifier) {
                    Some(m) => m,
                    None => continue,
                };

            let diag_line = diag.range.start.line as usize;

            let docblock = match find_docblock_above_line(content, diag_line) {
                Some(db) => db,
                None => continue,
            };

            if find_tag_line(&docblock, &mismatch).is_none() {
                continue;
            }

            // ── Action 1: Update tag type ───────────────────────────
            {
                let title = build_update_title(&mismatch);
                out.push(deferred_quickfix(
                    title,
                    "phpstan.fixPhpDocType.update",
                    uri,
                    &params.range,
                    vec![diag.clone()],
                    false,
                    serde_json::Value::Null,
                ));
            }

            // ── Action 2: Remove tag (preferred) ────────────────────
            {
                let title = build_remove_title(&mismatch);
                out.push(deferred_quickfix(
                    title,
                    "phpstan.fixPhpDocType.remove",
                    uri,
                    &params.range,
                    vec![diag.clone()],
                    true,
                    serde_json::Value::Null,
                ));
            }
        }
    }

    /// Resolve a "Fix PHPDoc type" code action by computing the full
    /// workspace edit.
    ///
    /// `action_kind` is either `"phpstan.fixPhpDocType.update"` or
    /// `"phpstan.fixPhpDocType.remove"`.
    pub(crate) fn resolve_fix_phpdoc_type(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let extra = &data.extra;
        let message = extra.get("diagnostic_message")?.as_str()?;
        let line = extra.get("diagnostic_line")?.as_u64()? as usize;
        let code = extra.get("diagnostic_code")?.as_str()?;

        let mismatch = parse_mismatch(message, code)?;
        let docblock = find_docblock_above_line(content, line)?;

        let is_update = data.action_kind == "phpstan.fixPhpDocType.update";

        let edit = if is_update {
            build_update_tag_edit(content, &docblock, &mismatch)?
        } else {
            build_remove_tag_edit(content, &docblock, &mismatch)?
        };

        let doc_uri: Uri = data.uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
    }
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a PHPDoc type mismatch diagnostic is stale.
///
/// A diagnostic is stale when the offending tag no longer contains the
/// PHPDoc type from the original message, or the tag has been removed
/// entirely.
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/stale.rs`.
pub(crate) fn is_fix_phpdoc_type_stale(
    content: &str,
    diag_line: usize,
    message: &str,
    identifier: &str,
) -> bool {
    let mismatch = match parse_mismatch(message, identifier) {
        Some(m) => m,
        None => return false,
    };

    let docblock = match find_docblock_above_line(content, diag_line) {
        Some(db) => db,
        None => {
            // No docblock found — the tag was removed, so the
            // diagnostic is stale.
            return true;
        }
    };

    match find_tag_line(&docblock, &mismatch) {
        Some((_, tag_text)) => {
            // The tag exists.  Check if it still contains the PHPDoc
            // type from the diagnostic message.
            !tag_text.contains(&mismatch.phpdoc_type)
        }
        None => {
            // The tag no longer exists — definitely stale.
            true
        }
    }
}

// ── Message parsing ─────────────────────────────────────────────────────────

/// Parse a PHPDoc type mismatch from a PHPStan diagnostic message.
///
/// Handles these message formats:
///
/// **`return.phpDocType`:**
/// - `PHPDoc tag @return with type {phpdoc} is incompatible with native type {native}.`
/// - `PHPDoc tag @return with type {phpdoc} is not subtype of native type {native}.`
///
/// **`parameter.phpDocType`:**
/// - `PHPDoc tag @param for parameter $name with type {phpdoc} is incompatible with native type {native}.`
/// - `PHPDoc tag @param for parameter $name with type {phpdoc} is not subtype of native type {native}.`
///
/// **`property.phpDocType`:**
/// - `PHPDoc tag @var for property Cls::$prop with type {phpdoc} is incompatible with native type {native}.`
/// - `PHPDoc tag @var for property Cls::$prop with type {phpdoc} is not subtype of native type {native}.`
fn parse_mismatch(message: &str, identifier: &str) -> Option<PhpDocMismatch> {
    match identifier {
        RETURN_PHPDOC_TYPE_ID => parse_return_mismatch(message),
        PARAM_PHPDOC_TYPE_ID => parse_param_mismatch(message),
        PROPERTY_PHPDOC_TYPE_ID => parse_property_mismatch(message),
        _ => None,
    }
}

/// Parse a `return.phpDocType` message.
///
/// Format: `PHPDoc tag @return with type {phpdoc} is (incompatible with|not subtype of) native type {native}.`
fn parse_return_mismatch(message: &str) -> Option<PhpDocMismatch> {
    let marker = "@return with type ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];

    let (phpdoc, native) = extract_types_from_rest(rest)?;

    Some(PhpDocMismatch {
        tag: "@return",
        phpdoc_type: phpdoc,
        native_type: native,
        param_name: None,
    })
}

/// Parse a `parameter.phpDocType` message.
///
/// Format: `PHPDoc tag @param for parameter $name with type {phpdoc} is ... native type {native}.`
fn parse_param_mismatch(message: &str) -> Option<PhpDocMismatch> {
    // Extract the parameter name.
    let param_marker = "@param for parameter ";
    let param_start = message.find(param_marker)? + param_marker.len();
    let param_rest = &message[param_start..];
    let param_end = param_rest.find(" with type ")?;
    let param_name = param_rest[..param_end].trim().to_string();

    // Extract the types.
    let type_marker = " with type ";
    let type_start = param_start + param_end + type_marker.len();
    let rest = &message[type_start..];

    let (phpdoc, native) = extract_types_from_rest(rest)?;

    Some(PhpDocMismatch {
        tag: "@param",
        phpdoc_type: phpdoc,
        native_type: native,
        param_name: Some(param_name),
    })
}

/// Parse a `property.phpDocType` message.
///
/// Format: `PHPDoc tag @var for property Cls::$prop with type {phpdoc} is ... native type {native}.`
///
/// Also handles the alternate formats:
/// - `{desc} for property Cls::$bar with type {phpdoc} is ...`
fn parse_property_mismatch(message: &str) -> Option<PhpDocMismatch> {
    let type_marker = " with type ";
    let type_start = message.find(type_marker)? + type_marker.len();
    let rest = &message[type_start..];

    let (phpdoc, native) = extract_types_from_rest(rest)?;

    Some(PhpDocMismatch {
        tag: "@var",
        phpdoc_type: phpdoc,
        native_type: native,
        param_name: None,
    })
}

/// Extract the PHPDoc and native type strings from the remainder of a
/// PHPStan diagnostic message.
///
/// Returns raw strings rather than `PhpType` because the PHPDoc type is
/// spliced back into docblock text during the fix edit, which requires the
/// original string form.
///
/// Handles both:
/// - `{phpdoc} is incompatible with native type {native}.`
/// - `{phpdoc} is not subtype of native type {native}.`
fn extract_types_from_rest(rest: &str) -> Option<(String, String)> {
    // Try "is incompatible with native type" first.
    let incompatible = " is incompatible with native type ";
    if let Some(pos) = rest.find(incompatible) {
        let phpdoc = rest[..pos].trim().to_string();
        let native = rest[pos + incompatible.len()..]
            .trim_end_matches('.')
            .trim()
            .to_string();
        if !phpdoc.is_empty() && !native.is_empty() {
            return Some((phpdoc, native));
        }
    }

    // Try "is not subtype of native type".
    let not_subtype = " is not subtype of native type ";
    if let Some(pos) = rest.find(not_subtype) {
        let phpdoc = rest[..pos].trim().to_string();
        let native = rest[pos + not_subtype.len()..]
            .trim_end_matches('.')
            .trim()
            .to_string();
        if !phpdoc.is_empty() && !native.is_empty() {
            return Some((phpdoc, native));
        }
    }

    None
}

// ── Tag finding ─────────────────────────────────────────────────────────────

/// Find the docblock line holding the tag the mismatch is about.
///
/// Returns the line's index within the docblock and its tag text (the
/// line without its `*` prefix and docblock delimiters).
fn find_tag_line<'d>(
    docblock: &'d DocblockAbove,
    mismatch: &PhpDocMismatch,
) -> Option<(usize, &'d str)> {
    docblock.text.lines().enumerate().find_map(|(i, line)| {
        let tag_text = strip_docblock_line_delimiters(line.trim());
        let after_tag = tag_text.strip_prefix(mismatch.tag)?.trim_start();
        let found = match mismatch.tag {
            // A `@param` line is the one naming the parameter.
            "@param" => mismatch
                .param_name
                .as_ref()
                .is_some_and(|name| after_tag.contains(name.as_str())),
            // Any `@return` or `@var` line is the one we're looking for.
            "@return" | "@var" => true,
            _ => false,
        };
        found.then_some((i, tag_text))
    })
}

// ── Edit builders ───────────────────────────────────────────────────────────

/// Build a `TextEdit` that updates the tag's type to the native type.
fn build_update_tag_edit(
    content: &str,
    docblock: &DocblockAbove,
    mismatch: &PhpDocMismatch,
) -> Option<TextEdit> {
    let (tag_line_idx, _) = find_tag_line(docblock, mismatch)?;
    let mut lines: Vec<&str> = docblock.text.lines().collect();
    let new_line = replace_type_in_tag_line(lines[tag_line_idx], mismatch)?;
    lines[tag_line_idx] = &new_line;
    Some(docblock.rewrite_edit(content, &lines))
}

/// Replace the PHPDoc type in a tag line with the native type.
///
/// Handles lines like:
/// - `     * @return SomeType description`
/// - `     * @param SomeType $name description`
/// - `     * @var SomeType description`
fn replace_type_in_tag_line(line: &str, mismatch: &PhpDocMismatch) -> Option<String> {
    let tag = mismatch.tag;
    let tag_pos = line.find(tag)?;
    let after_tag_start = tag_pos + tag.len();
    let after_tag = &line[after_tag_start..];

    // The type is the first non-whitespace token after the tag.
    let whitespace_len = after_tag.len() - after_tag.trim_start().len();
    let trimmed_after = after_tag.trim_start();

    // Find where the type ends, respecting `<>`, `{}`, `()` nesting.
    let (type_token, _) = crate::docblock::type_strings::split_type_token(trimmed_after);
    let type_end = type_token.len();

    let type_start_in_line = after_tag_start + whitespace_len;
    let type_end_in_line = type_start_in_line + type_end;

    let mut result = String::with_capacity(line.len());
    result.push_str(&line[..type_start_in_line]);
    result.push_str(&mismatch.native_type);
    result.push_str(&line[type_end_in_line..]);

    Some(result)
}

/// Build a `TextEdit` that removes the tag line from the docblock.
///
/// If the docblock would become empty (only `/**` and `*/` with maybe
/// blank `*` lines), the entire docblock is removed.
fn build_remove_tag_edit(
    content: &str,
    docblock: &DocblockAbove,
    mismatch: &PhpDocMismatch,
) -> Option<TextEdit> {
    let doc_lines: Vec<&str> = docblock.text.lines().collect();
    let (tag_line_idx, _) = find_tag_line(docblock, mismatch)?;

    let mut lines_to_remove = vec![tag_line_idx];

    // Also remove orphaned blank `*` separator lines.
    // If the line before the removed tag is a blank `* ` and the line
    // after is `*/` or another blank, remove the blank too.
    if tag_line_idx > 0 && is_blank_doc_line(doc_lines[tag_line_idx - 1]) {
        let next_idx = tag_line_idx + 1;
        if next_idx >= doc_lines.len()
            || doc_lines[next_idx].trim() == "*/"
            || is_blank_doc_line(doc_lines[next_idx])
        {
            lines_to_remove.push(tag_line_idx - 1);
        }
    }

    Some(docblock.remove_lines_edit(content, &lines_to_remove))
}

/// Check if a docblock line is a blank `*` line (no content besides
/// the `*` prefix and whitespace).
fn is_blank_doc_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed == "*" {
        return true;
    }
    let stripped = trimmed.trim_start_matches('*').trim();
    stripped.is_empty() && !trimmed.contains("/**") && !trimmed.contains("*/")
}

// ── Title builders ──────────────────────────────────────────────────────────

/// Build the title for the "update type" action.
fn build_update_title(mismatch: &PhpDocMismatch) -> String {
    match mismatch.tag {
        "@return" => format!("Update @return type to `{}`", mismatch.native_type),
        "@param" => {
            let name = mismatch.param_name.as_deref().unwrap_or("parameter");
            format!("Update @param {} type to `{}`", name, mismatch.native_type)
        }
        "@var" => format!("Update @var type to `{}`", mismatch.native_type),
        _ => "Update PHPDoc type".to_string(),
    }
}

/// Build the title for the "remove tag" action.
fn build_remove_title(mismatch: &PhpDocMismatch) -> String {
    match mismatch.tag {
        "@return" => "Remove @return tag".to_string(),
        "@param" => {
            let name = mismatch.param_name.as_deref().unwrap_or("parameter");
            format!("Remove @param {} tag", name)
        }
        "@var" => "Remove @var tag".to_string(),
        _ => "Remove PHPDoc tag".to_string(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Message parsing ─────────────────────────────────────────────

    #[test]
    fn parses_return_incompatible() {
        let msg =
            "PHPDoc tag @return with type string|false is incompatible with native type string.";
        let m = parse_mismatch(msg, RETURN_PHPDOC_TYPE_ID).unwrap();
        assert_eq!(m.tag, "@return");
        assert_eq!(m.phpdoc_type, "string|false");
        assert_eq!(m.native_type, "string");
        assert!(m.param_name.is_none());
    }

    #[test]
    fn parses_return_not_subtype() {
        let msg = "PHPDoc tag @return with type array<string, mixed> is not subtype of native type array.";
        let m = parse_mismatch(msg, RETURN_PHPDOC_TYPE_ID).unwrap();
        assert_eq!(m.tag, "@return");
        assert_eq!(m.phpdoc_type, "array<string, mixed>");
        assert_eq!(m.native_type, "array");
        assert!(m.param_name.is_none());
    }

    #[test]
    fn parses_param_incompatible() {
        let msg = "PHPDoc tag @param for parameter $name with type int is incompatible with native type string.";
        let m = parse_mismatch(msg, PARAM_PHPDOC_TYPE_ID).unwrap();
        assert_eq!(m.tag, "@param");
        assert_eq!(m.phpdoc_type, "int");
        assert_eq!(m.native_type, "string");
        assert_eq!(m.param_name.as_deref(), Some("$name"));
    }

    #[test]
    fn parses_param_not_subtype() {
        let msg = "PHPDoc tag @param for parameter $items with type list<string> is not subtype of native type array.";
        let m = parse_mismatch(msg, PARAM_PHPDOC_TYPE_ID).unwrap();
        assert_eq!(m.tag, "@param");
        assert_eq!(m.phpdoc_type, "list<string>");
        assert_eq!(m.native_type, "array");
        assert_eq!(m.param_name.as_deref(), Some("$items"));
    }

    #[test]
    fn parses_property_incompatible() {
        let msg = "PHPDoc tag @var for property Foo::$bar with type int is incompatible with native type string.";
        let m = parse_mismatch(msg, PROPERTY_PHPDOC_TYPE_ID).unwrap();
        assert_eq!(m.tag, "@var");
        assert_eq!(m.phpdoc_type, "int");
        assert_eq!(m.native_type, "string");
        assert!(m.param_name.is_none());
    }

    #[test]
    fn parses_property_not_subtype() {
        let msg = "PHPDoc tag @var for property App\\Models\\User::$email with type non-empty-string is not subtype of native type string.";
        let m = parse_mismatch(msg, PROPERTY_PHPDOC_TYPE_ID).unwrap();
        assert_eq!(m.tag, "@var");
        assert_eq!(m.phpdoc_type, "non-empty-string");
        assert_eq!(m.native_type, "string");
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Method Foo::bar() should return string but returns int.";
        assert!(parse_mismatch(msg, RETURN_PHPDOC_TYPE_ID).is_none());
    }

    #[test]
    fn returns_none_for_wrong_identifier() {
        let msg = "PHPDoc tag @return with type int is incompatible with native type string.";
        assert!(parse_mismatch(msg, "some.other.id").is_none());
    }

    // ── Docblock discovery ──────────────────────────────────────────

    #[test]
    fn finds_docblock_above_function() {
        let content = "<?php\n/**\n * @return int\n */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 4).unwrap();
        assert!(db.text.contains("@return int"));
    }

    #[test]
    fn finds_docblock_with_attribute_between() {
        let content = "<?php\n/**\n * @return int\n */\n#[SomeAttr]\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 5).unwrap();
        assert!(db.text.contains("@return int"));
    }

    #[test]
    fn no_docblock_returns_none() {
        let content = "<?php\nfunction foo(): string {}";
        assert!(find_docblock_above_line(content, 1).is_none());
    }

    // ── Tag finding ─────────────────────────────────────────────────

    #[test]
    fn finds_return_tag() {
        let content = "<?php\n/**\n * @return int\n */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 4).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let (_, found) = find_tag_line(&db, &mismatch).unwrap();
        assert!(found.contains("@return"));
    }

    #[test]
    fn finds_param_tag_by_name() {
        let content = "<?php\n/**\n * @param int $a\n * @param string $b\n */\nfunction foo(string $a, int $b) {}";
        let db = find_docblock_above_line(content, 5).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@param",
            phpdoc_type: "string".to_string(),
            native_type: "int".to_string(),
            param_name: Some("$b".to_string()),
        };
        let (_, found) = find_tag_line(&db, &mismatch).unwrap();
        assert!(found.contains("$b"));
    }

    #[test]
    fn finds_var_tag() {
        let content =
            "<?php\nclass Foo {\n    /**\n     * @var int\n     */\n    public string $bar;\n}";
        let db = find_docblock_above_line(content, 5).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@var",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        assert!(find_tag_line(&db, &mismatch).is_some());
    }

    // ── Update edit ─────────────────────────────────────────────────

    #[test]
    fn updates_return_type() {
        let content = "<?php\n/**\n * @return int\n */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 4).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_update_tag_edit(content, &db, &mismatch).unwrap();
        assert!(edit.new_text.contains("@return string"));
        assert!(!edit.new_text.contains("@return int"));
    }

    #[test]
    fn updates_param_type() {
        let content = "<?php\n/**\n * @param int $name\n */\nfunction foo(string $name) {}";
        let db = find_docblock_above_line(content, 4).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@param",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: Some("$name".to_string()),
        };
        let edit = build_update_tag_edit(content, &db, &mismatch).unwrap();
        assert!(edit.new_text.contains("@param string $name"));
    }

    #[test]
    fn updates_var_type() {
        let content =
            "<?php\nclass Foo {\n    /**\n     * @var int\n     */\n    public string $bar;\n}";
        let db = find_docblock_above_line(content, 5).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@var",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_update_tag_edit(content, &db, &mismatch).unwrap();
        assert!(edit.new_text.contains("@var string"));
        assert!(!edit.new_text.contains("@var int"));
    }

    #[test]
    fn preserves_description_on_update() {
        let content =
            "<?php\n/**\n * @return int The user's age\n */\nfunction getAge(): string {}";
        let db = find_docblock_above_line(content, 4).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_update_tag_edit(content, &db, &mismatch).unwrap();
        assert!(edit.new_text.contains("@return string The user's age"));
    }

    #[test]
    fn preserves_param_description_on_update() {
        let content =
            "<?php\n/**\n * @param int $id The user ID\n */\nfunction find(string $id) {}";
        let db = find_docblock_above_line(content, 4).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@param",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: Some("$id".to_string()),
        };
        let edit = build_update_tag_edit(content, &db, &mismatch).unwrap();
        assert!(edit.new_text.contains("@param string $id The user ID"));
    }

    // ── Remove edit ─────────────────────────────────────────────────

    #[test]
    fn removes_return_tag() {
        let content = "<?php\n/**\n * Summary.\n *\n * @return int\n */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 6).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_remove_tag_edit(content, &db, &mismatch).unwrap();
        assert!(!edit.new_text.contains("@return"));
        assert!(edit.new_text.contains("Summary."));
    }

    #[test]
    fn removes_param_tag() {
        let content = "<?php\n/**\n * Summary.\n *\n * @param string $a\n * @param int $b\n */\nfunction foo(string $a, string $b) {}";
        let db = find_docblock_above_line(content, 7).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@param",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: Some("$b".to_string()),
        };
        let edit = build_remove_tag_edit(content, &db, &mismatch).unwrap();
        assert!(!edit.new_text.contains("$b"));
        assert!(edit.new_text.contains("@param string $a"));
    }

    #[test]
    fn removes_entire_docblock_when_only_tag() {
        let content = "<?php\n/**\n * @return int\n */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 4).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_remove_tag_edit(content, &db, &mismatch).unwrap();
        assert!(
            edit.new_text.is_empty(),
            "docblock should be removed entirely"
        );
    }

    #[test]
    fn removes_var_tag_keeps_description() {
        let content = "<?php\nclass Foo {\n    /**\n     * The bar property.\n     *\n     * @var int\n     */\n    public string $bar;\n}";
        let db = find_docblock_above_line(content, 7).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@var",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_remove_tag_edit(content, &db, &mismatch).unwrap();
        assert!(!edit.new_text.contains("@var"));
        assert!(edit.new_text.contains("The bar property."));
    }

    #[test]
    fn removes_orphaned_blank_separator() {
        let content = "<?php\n/**\n * Summary.\n *\n * @return int\n */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 6).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_remove_tag_edit(content, &db, &mismatch).unwrap();
        // The blank ` *` line before @return should also be removed
        // since it would be orphaned (followed by `*/`).
        let lines: Vec<&str> = edit.new_text.lines().collect();
        // Should not end with a blank `*` line before `*/`.
        if lines.len() >= 2 {
            let before_close = lines[lines.len() - 2].trim();
            assert_ne!(
                before_close.trim_start_matches('*').trim(),
                "",
                "orphaned blank * line should be removed"
            );
        }
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let content = "<?php\r\n/**\r\n * Summary.\r\n *\r\n * @param int $a\r\n * @return int\r\n */\r\nfunction foo(string $a): string {}";
        let db = find_docblock_above_line(content, 7).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_update_tag_edit(content, &db, &mismatch).unwrap();
        assert_eq!(
            edit.new_text,
            "/**\r\n * Summary.\r\n *\r\n * @param int $a\r\n * @return string\r\n */\r\n"
        );
        let edit = build_remove_tag_edit(content, &db, &mismatch).unwrap();
        assert_eq!(
            edit.new_text,
            "/**\r\n * Summary.\r\n *\r\n * @param int $a\r\n */\r\n"
        );
    }

    // ── Stale detection ─────────────────────────────────────────────

    #[test]
    fn stale_when_return_tag_removed() {
        let content = "<?php\n/**\n * Summary.\n */\nfunction foo(): string {}";
        let msg = "PHPDoc tag @return with type int is incompatible with native type string.";
        assert!(is_fix_phpdoc_type_stale(
            content,
            4,
            msg,
            RETURN_PHPDOC_TYPE_ID
        ));
    }

    #[test]
    fn stale_when_return_type_changed() {
        let content = "<?php\n/**\n * @return string\n */\nfunction foo(): string {}";
        let msg = "PHPDoc tag @return with type int is incompatible with native type string.";
        assert!(is_fix_phpdoc_type_stale(
            content,
            4,
            msg,
            RETURN_PHPDOC_TYPE_ID
        ));
    }

    #[test]
    fn not_stale_when_return_type_still_wrong() {
        let content = "<?php\n/**\n * @return int\n */\nfunction foo(): string {}";
        let msg = "PHPDoc tag @return with type int is incompatible with native type string.";
        assert!(!is_fix_phpdoc_type_stale(
            content,
            4,
            msg,
            RETURN_PHPDOC_TYPE_ID
        ));
    }

    #[test]
    fn stale_when_param_tag_removed() {
        let content = "<?php\n/**\n * Summary.\n */\nfunction foo(string $name) {}";
        let msg = "PHPDoc tag @param for parameter $name with type int is incompatible with native type string.";
        assert!(is_fix_phpdoc_type_stale(
            content,
            4,
            msg,
            PARAM_PHPDOC_TYPE_ID
        ));
    }

    #[test]
    fn not_stale_when_param_type_still_wrong() {
        let content = "<?php\n/**\n * @param int $name\n */\nfunction foo(string $name) {}";
        let msg = "PHPDoc tag @param for parameter $name with type int is incompatible with native type string.";
        assert!(!is_fix_phpdoc_type_stale(
            content,
            4,
            msg,
            PARAM_PHPDOC_TYPE_ID
        ));
    }

    #[test]
    fn stale_when_no_docblock() {
        let content = "<?php\nfunction foo(): string {}";
        let msg = "PHPDoc tag @return with type int is incompatible with native type string.";
        assert!(is_fix_phpdoc_type_stale(
            content,
            1,
            msg,
            RETURN_PHPDOC_TYPE_ID
        ));
    }

    #[test]
    fn stale_when_var_tag_removed() {
        let content =
            "<?php\nclass Foo {\n    /**\n     * Description.\n     */\n    public string $bar;\n}";
        let msg = "PHPDoc tag @var for property Foo::$bar with type int is incompatible with native type string.";
        assert!(is_fix_phpdoc_type_stale(
            content,
            5,
            msg,
            PROPERTY_PHPDOC_TYPE_ID
        ));
    }

    // ── Title builders ──────────────────────────────────────────────

    #[test]
    fn update_title_for_return() {
        let m = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: None,
        };
        assert_eq!(build_update_title(&m), "Update @return type to `string`");
    }

    #[test]
    fn update_title_for_param() {
        let m = PhpDocMismatch {
            tag: "@param",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: Some("$name".into()),
        };
        assert_eq!(
            build_update_title(&m),
            "Update @param $name type to `string`"
        );
    }

    #[test]
    fn remove_title_for_return() {
        let m = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: None,
        };
        assert_eq!(build_remove_title(&m), "Remove @return tag");
    }

    #[test]
    fn remove_title_for_param() {
        let m = PhpDocMismatch {
            tag: "@param",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: Some("$id".into()),
        };
        assert_eq!(build_remove_title(&m), "Remove @param $id tag");
    }

    #[test]
    fn remove_title_for_var() {
        let m = PhpDocMismatch {
            tag: "@var",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: None,
        };
        assert_eq!(build_remove_title(&m), "Remove @var tag");
    }

    // ── Type replacement ────────────────────────────────────────────

    #[test]
    fn replaces_simple_type() {
        let line = "     * @return int";
        let m = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: None,
        };
        let result = replace_type_in_tag_line(line, &m).unwrap();
        assert_eq!(result, "     * @return string");
    }

    #[test]
    fn replaces_type_preserving_description() {
        let line = "     * @return int The age";
        let m = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: None,
        };
        let result = replace_type_in_tag_line(line, &m).unwrap();
        assert_eq!(result, "     * @return string The age");
    }

    #[test]
    fn replaces_param_type() {
        let line = "     * @param int $id The user ID";
        let m = PhpDocMismatch {
            tag: "@param",
            phpdoc_type: "int".into(),
            native_type: "string".into(),
            param_name: Some("$id".into()),
        };
        let result = replace_type_in_tag_line(line, &m).unwrap();
        assert_eq!(result, "     * @param string $id The user ID");
    }

    #[test]
    fn replaces_union_type() {
        let line = "     * @return string|false";
        let m = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "string|false".into(),
            native_type: "string".into(),
            param_name: None,
        };
        let result = replace_type_in_tag_line(line, &m).unwrap();
        assert_eq!(result, "     * @return string");
    }

    #[test]
    fn replaces_generic_type() {
        let line = "     * @return array<string, mixed>";
        let m = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "array<string, mixed>".into(),
            native_type: "array".into(),
            param_name: None,
        };
        let result = replace_type_in_tag_line(line, &m).unwrap();
        assert_eq!(result, "     * @return array");
    }

    // ── Single-line docblock ────────────────────────────────────────

    #[test]
    fn handles_single_line_docblock_remove() {
        let content = "<?php\n/** @return int */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 2).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_remove_tag_edit(content, &db, &mismatch).unwrap();
        assert!(
            edit.new_text.is_empty(),
            "single-line docblock with only @return should be removed entirely"
        );
    }

    #[test]
    fn handles_single_line_docblock_update() {
        let content = "<?php\n/** @return int */\nfunction foo(): string {}";
        let db = find_docblock_above_line(content, 2).unwrap();
        let mismatch = PhpDocMismatch {
            tag: "@return",
            phpdoc_type: "int".to_string(),
            native_type: "string".to_string(),
            param_name: None,
        };
        let edit = build_update_tag_edit(content, &db, &mismatch).unwrap();
        assert!(edit.new_text.contains("@return string"));
    }

    // ── extract_types_from_rest ─────────────────────────────────────

    #[test]
    fn extracts_incompatible_types() {
        let rest = "int is incompatible with native type string.";
        let (phpdoc, native) = extract_types_from_rest(rest).unwrap();
        assert_eq!(phpdoc, "int");
        assert_eq!(native, "string");
    }

    #[test]
    fn extracts_not_subtype_types() {
        let rest = "array<string, mixed> is not subtype of native type array.";
        let (phpdoc, native) = extract_types_from_rest(rest).unwrap();
        assert_eq!(phpdoc, "array<string, mixed>");
        assert_eq!(native, "array");
    }

    #[test]
    fn returns_none_for_no_match() {
        let rest = "something completely different.";
        assert!(extract_types_from_rest(rest).is_none());
    }
}
