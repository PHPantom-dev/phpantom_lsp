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

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::text_position::{offset_to_position, ranges_overlap};

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
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags: Vec<Diagnostic> = {
            let cache = self.phpstan_tool.last_diags.lock();
            cache.get(uri).cloned().unwrap_or_default()
        };

        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            if !ALL_IDS.contains(&identifier) {
                continue;
            }

            let mismatch = match parse_mismatch(&diag.message, identifier) {
                Some(m) => m,
                None => continue,
            };

            let diag_line = diag.range.start.line as usize;

            // Find the docblock above the diagnostic line.
            let docblock = match find_docblock_above_line(content, diag_line) {
                Some(db) => db,
                None => continue,
            };

            // Validate that the tag exists in the docblock.
            if find_tag_line_in_docblock(&docblock, &mismatch).is_none() {
                continue;
            }

            let extra = serde_json::json!({
                "diagnostic_message": diag.message,
                "diagnostic_line": diag_line,
                "diagnostic_code": identifier,
            });

            // ── Action 1: Update tag type ───────────────────────────
            {
                let title = build_update_title(&mismatch);
                let data = make_code_action_data(
                    "phpstan.fixPhpDocType.update",
                    uri,
                    &params.range,
                    extra.clone(),
                );

                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: None,
                    command: None,
                    is_preferred: Some(false),
                    disabled: None,
                    data: Some(data),
                }));
            }

            // ── Action 2: Remove tag (preferred) ────────────────────
            {
                let title = build_remove_title(&mismatch);
                let data = make_code_action_data(
                    "phpstan.fixPhpDocType.remove",
                    uri,
                    &params.range,
                    extra,
                );

                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: None,
                    command: None,
                    is_preferred: Some(true),
                    disabled: None,
                    data: Some(data),
                }));
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

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, vec![edit]);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a PHPDoc type mismatch diagnostic is stale.
///
/// A diagnostic is stale when the offending tag no longer contains the
/// PHPDoc type from the original message, or the tag has been removed
/// entirely.
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/mod.rs`.
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

    match find_tag_line_in_docblock(&docblock, &mismatch) {
        Some(tag_text) => {
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
    // Find "with type " — the types come after it.
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

// ── Docblock discovery ──────────────────────────────────────────────────────

/// Information about a docblock found above a given line.
struct DocblockAbove {
    /// Byte offset of the start of the docblock (first char of `/**` line,
    /// including leading whitespace).
    start: usize,
    /// Byte offset just past the end of the docblock (past `*/` line,
    /// including trailing newline).
    end: usize,
    /// The raw docblock text including indentation.
    text: String,
}

/// Find the docblock immediately above the given line.
///
/// The diagnostic line is the function/method/property signature.  The
/// docblock (if any) sits directly above it, possibly separated by
/// blank lines or attribute lines.
fn find_docblock_above_line(content: &str, line: usize) -> Option<DocblockAbove> {
    let lines: Vec<&str> = content.lines().collect();
    if line == 0 || line > lines.len() {
        return None;
    }

    // Walk backward from the line before the diagnostic to find `*/`.
    let mut doc_end_line = None;
    for i in (0..line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.ends_with("*/") {
            doc_end_line = Some(i);
            break;
        }
        // Attributes (#[...]) can appear between docblock and declaration.
        if trimmed.starts_with("#[") {
            continue;
        }
        // Anything else means no docblock.
        break;
    }

    let end_line = doc_end_line?;

    // Walk backward from end_line to find `/**`.
    let mut doc_start_line = None;
    for i in (0..=end_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.contains("/**") {
            doc_start_line = Some(i);
            break;
        }
        // Should be a `*`-prefixed line or end-of-docblock.
        if !trimmed.starts_with('*') && !trimmed.ends_with("*/") {
            break;
        }
    }

    let start_line = doc_start_line?;

    // Convert line numbers to byte offsets.
    let mut byte_offset = 0;
    let mut start_byte = 0;
    let mut end_byte = 0;
    for (i, line_text) in lines.iter().enumerate() {
        if i == start_line {
            start_byte = byte_offset;
        }
        byte_offset += line_text.len() + 1; // +1 for newline
        if i == end_line {
            end_byte = byte_offset; // include trailing newline
        }
    }

    let text = content
        .get(start_byte..end_byte.min(content.len()))
        .unwrap_or("")
        .to_string();

    Some(DocblockAbove {
        start: start_byte,
        end: end_byte.min(content.len()),
        text,
    })
}

// ── Tag finding ─────────────────────────────────────────────────────────────

/// Find the tag line in the docblock that matches the mismatch.
///
/// Returns the trimmed content of the tag line (the `* @tag type ...`
/// portion) if found, or `None`.
fn find_tag_line_in_docblock(
    docblock: &DocblockAbove,
    mismatch: &PhpDocMismatch,
) -> Option<String> {
    let tag_prefix = mismatch.tag; // e.g. "@return", "@param", "@var"

    for line in docblock.text.lines() {
        let mut trimmed = line.trim();
        // Strip docblock delimiters.
        if let Some(inner) = trimmed.strip_prefix("/**") {
            trimmed = inner.trim_start();
        }
        if let Some(inner) = trimmed.strip_suffix("*/") {
            trimmed = inner.trim_end();
        }
        trimmed = trimmed.trim_start_matches('*').trim();

        if !trimmed.starts_with(tag_prefix) {
            continue;
        }

        let after_tag = trimmed[tag_prefix.len()..].trim_start();

        match mismatch.tag {
            "@param" => {
                // For @param, check that this is the right parameter.
                // Format: `@param type $name description`
                // The param name could be the second or later token.
                if let Some(ref param_name) = mismatch.param_name
                    && after_tag.contains(param_name.as_str())
                {
                    return Some(trimmed.to_string());
                }
            }
            "@return" | "@var" => {
                // Any @return or @var line is the one we're looking for.
                return Some(trimmed.to_string());
            }
            _ => {}
        }
    }

    None
}

/// Find the line index (within the docblock lines) that contains the
/// matching tag.
fn find_tag_line_index(docblock: &DocblockAbove, mismatch: &PhpDocMismatch) -> Option<usize> {
    let tag_prefix = mismatch.tag;

    for (i, line) in docblock.text.lines().enumerate() {
        let mut trimmed = line.trim();
        if let Some(inner) = trimmed.strip_prefix("/**") {
            trimmed = inner.trim_start();
        }
        if let Some(inner) = trimmed.strip_suffix("*/") {
            trimmed = inner.trim_end();
        }
        trimmed = trimmed.trim_start_matches('*').trim();

        if !trimmed.starts_with(tag_prefix) {
            continue;
        }

        let after_tag = trimmed[tag_prefix.len()..].trim_start();

        match mismatch.tag {
            "@param" => {
                if let Some(ref param_name) = mismatch.param_name
                    && after_tag.contains(param_name.as_str())
                {
                    return Some(i);
                }
            }
            "@return" | "@var" => {
                return Some(i);
            }
            _ => {}
        }
    }

    None
}

// ── Edit builders ───────────────────────────────────────────────────────────

/// Build a `TextEdit` that updates the tag's type to the native type.
fn build_update_tag_edit(
    content: &str,
    docblock: &DocblockAbove,
    mismatch: &PhpDocMismatch,
) -> Option<TextEdit> {
    let doc_lines: Vec<&str> = docblock.text.lines().collect();
    let tag_line_idx = find_tag_line_index(docblock, mismatch)?;

    let original_line = doc_lines[tag_line_idx];

    // Build the replacement line by substituting the PHPDoc type with
    // the native type.
    let new_line = replace_type_in_tag_line(original_line, mismatch)?;

    // Rebuild the docblock with the replaced line.
    let mut new_lines: Vec<&str> = Vec::with_capacity(doc_lines.len());
    let new_line_ref: String = new_line;
    for (i, line) in doc_lines.iter().enumerate() {
        if i == tag_line_idx {
            // Will be pushed below.
            continue;
        }
        new_lines.push(line);
    }

    // We need to insert the new line at the right position.
    // Rebuild manually to get ownership right.
    let mut result_lines: Vec<String> = Vec::with_capacity(doc_lines.len());
    for (i, line) in doc_lines.iter().enumerate() {
        if i == tag_line_idx {
            result_lines.push(new_line_ref.clone());
        } else {
            result_lines.push((*line).to_string());
        }
    }

    let mut new_text = result_lines.join("\n");
    if docblock.text.ends_with('\n') && !new_text.ends_with('\n') {
        new_text.push('\n');
    }

    let start = offset_to_position(content, docblock.start);
    let end = offset_to_position(content, docblock.end);

    Some(TextEdit {
        range: Range { start, end },
        new_text,
    })
}

/// Replace the PHPDoc type in a tag line with the native type.
///
/// Handles lines like:
/// - `     * @return SomeType description`
/// - `     * @param SomeType $name description`
/// - `     * @var SomeType description`
fn replace_type_in_tag_line(line: &str, mismatch: &PhpDocMismatch) -> Option<String> {
    // Find the tag in the line.
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

    // Build the new line: everything before the type + new type +
    // everything after the type.
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
    let tag_line_idx = find_tag_line_index(docblock, mismatch)?;

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

    lines_to_remove.sort();
    lines_to_remove.dedup();

    // Build new docblock text.
    let new_lines: Vec<&str> = doc_lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !lines_to_remove.contains(i))
        .map(|(_, l)| *l)
        .collect();

    // Check if the docblock is essentially empty after removal.
    let has_content = new_lines.iter().any(|l| {
        let mut t = l.trim();
        if let Some(inner) = t.strip_prefix("/**") {
            t = inner.trim_start();
        }
        if let Some(inner) = t.strip_suffix("*/") {
            t = inner.trim_end();
        }
        t = t.trim_start_matches('*').trim();
        !t.is_empty()
    });

    let new_text = if !has_content && new_lines.len() <= 3 {
        // Docblock is empty after removal — remove it entirely.
        String::new()
    } else {
        let mut text = new_lines.join("\n");
        if docblock.text.ends_with('\n') && !text.ends_with('\n') {
            text.push('\n');
        }
        text
    };

    let start = offset_to_position(content, docblock.start);
    let end = offset_to_position(content, docblock.end);

    Some(TextEdit {
        range: Range { start, end },
        new_text,
    })
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
        let found = find_tag_line_in_docblock(&db, &mismatch).unwrap();
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
        let found = find_tag_line_in_docblock(&db, &mismatch).unwrap();
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
        assert!(find_tag_line_in_docblock(&db, &mismatch).is_some());
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
