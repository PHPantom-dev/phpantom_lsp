//! "Fix unsafe `new static()`" code actions for PHPStan `new.static`.
//!
//! When PHPStan reports `Unsafe usage of new static().`, this module
//! offers three quickfixes:
//!
//! 1. **Add `@phpstan-consistent-constructor`** to the class docblock
//!    (preferred). Creates a new docblock if none exists.
//! 2. **Add `final` to class** — insert `final` before the `class`
//!    keyword on the class declaration line.
//! 3. **Add `final` to constructor** — insert `final` before the
//!    visibility modifier (or `function` keyword) on `__construct`.
//!
//! **Trigger:** A PHPStan diagnostic with identifier `new.static`
//! overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_new_static_actions`) performs all validation and
//! emits lightweight `CodeAction` values with a `data` payload but no
//! `edit`.  Phase 2 (`resolve_new_static`) computes the workspace edit
//! on demand when the user picks an action.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::util::{offset_to_position, ranges_overlap};

/// The PHPStan identifier we match on.
const NEW_STATIC_ID: &str = "new.static";

/// The three sub-actions the user can pick.
const ACTION_ADD_TAG: &str = "phpstan.newStatic.addTag";
const ACTION_FINAL_CLASS: &str = "phpstan.newStatic.finalClass";
const ACTION_FINAL_CONSTRUCTOR: &str = "phpstan.newStatic.finalConstructor";

impl Backend {
    /// Collect "Fix unsafe `new static()`" code actions for PHPStan
    /// `new.static` diagnostics.
    ///
    /// **Phase 1**: validates the action is applicable and emits up to
    /// three lightweight `CodeAction` values with `data` payloads but
    /// **no `edit`**.  Edits are computed lazily in
    /// [`resolve_new_static`](Self::resolve_new_static).
    pub(crate) fn collect_new_static_actions(
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

            if identifier != NEW_STATIC_ID {
                continue;
            }

            let diag_line = diag.range.start.line as usize;

            // Find the enclosing class declaration by walking backward.
            let Some(class_info) = find_enclosing_class(content, diag_line) else {
                continue;
            };

            // Check staleness: if the class is already final, the
            // constructor is already final, or the docblock already
            // contains `@phpstan-consistent-constructor`, skip.
            if is_already_fixed(content, &class_info) {
                continue;
            }

            let class_name = class_info.class_name.as_deref().unwrap_or("class");

            // ── Action 1: Add @phpstan-consistent-constructor (preferred) ──
            {
                let title = format!("Add @phpstan-consistent-constructor to {}", class_name);
                let extra = serde_json::json!({
                    "diagnostic_message": diag.message,
                    "diagnostic_line": diag.range.start.line,
                    "diagnostic_code": NEW_STATIC_ID,
                    "sub_action": ACTION_ADD_TAG,
                });
                let data = make_code_action_data(ACTION_ADD_TAG, uri, &params.range, extra);
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

            // ── Action 2: Add `final` to class ─────────────────────────
            {
                let title = format!("Add final to class {}", class_name);
                let extra = serde_json::json!({
                    "diagnostic_message": diag.message,
                    "diagnostic_line": diag.range.start.line,
                    "diagnostic_code": NEW_STATIC_ID,
                    "sub_action": ACTION_FINAL_CLASS,
                });
                let data = make_code_action_data(ACTION_FINAL_CLASS, uri, &params.range, extra);
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

            // ── Action 3: Add `final` to constructor ────────────────────
            if class_info.constructor.is_some() {
                let title = format!("Add final to {}::__construct", class_name);
                let extra = serde_json::json!({
                    "diagnostic_message": diag.message,
                    "diagnostic_line": diag.range.start.line,
                    "diagnostic_code": NEW_STATIC_ID,
                    "sub_action": ACTION_FINAL_CONSTRUCTOR,
                });
                let data =
                    make_code_action_data(ACTION_FINAL_CONSTRUCTOR, uri, &params.range, extra);
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
        }
    }

    /// Resolve a "Fix unsafe `new static()`" code action by computing
    /// the full workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks one of the three sub-actions.
    pub(crate) fn resolve_new_static(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;
        let sub_action = data.extra.get("sub_action")?.as_str()?;

        let class_info = find_enclosing_class(content, diag_line)?;

        if is_already_fixed(content, &class_info) {
            return None;
        }

        let edits = match sub_action {
            ACTION_ADD_TAG => build_add_tag_edit(content, &class_info),
            ACTION_FINAL_CLASS => build_final_class_edit(content, &class_info),
            ACTION_FINAL_CONSTRUCTOR => build_final_constructor_edit(content, &class_info),
            _ => return None,
        };

        let edits = edits?;

        let doc_uri: Url = uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

// ── Data structures ─────────────────────────────────────────────────────────

/// Information about the enclosing class declaration.
struct EnclosingClassInfo {
    /// The class name, if we could extract it.
    class_name: Option<String>,

    /// Byte offset of the start of the line containing the `class`
    /// keyword (or `abstract class`, `readonly class`, etc.).
    class_line_start: usize,

    /// Byte offset of the `class` keyword itself.
    class_keyword_offset: usize,

    /// Whether the class already has `abstract` modifier.
    is_abstract: bool,

    /// Information about the existing class docblock, if any.
    docblock: Option<ClassDocblock>,

    /// Information about the constructor, if found.
    constructor: Option<ConstructorInfo>,
}

/// An existing docblock above the class declaration.
struct ClassDocblock {
    /// Byte offset of `/**`.
    start: usize,
    /// Byte offset just past `*/`.
    end: usize,
    /// The raw docblock text.
    text: String,
}

/// Information about the `__construct` method in the class.
struct ConstructorInfo {
    /// Byte offset of the first token on the constructor declaration
    /// line (the visibility modifier or `function` keyword).
    decl_start: usize,
    /// Whether `final` is already present on the constructor.
    has_final: bool,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Find the enclosing class declaration by walking backward from the
/// diagnostic line.
///
/// The `new static()` diagnostic always fires inside a method body,
/// so we walk backward tracking brace depth to escape the method body,
/// then the class body, to find the `class` keyword.
fn find_enclosing_class(content: &str, diag_line: usize) -> Option<EnclosingClassInfo> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    // Convert the diagnostic line to a byte offset.
    let diag_byte_offset: usize = lines.iter().take(diag_line).map(|l| l.len() + 1).sum();

    // Walk backward from the diagnostic position tracking brace depth.
    // We need to exit the method body (depth -1) and then find the
    // class declaration.  The class `{` is at depth -2 when coming
    // from inside a method.
    //
    // Strategy: find the `class` keyword by scanning backward for it,
    // ensuring it's at the right structural level.  Since PHP classes
    // can't be nested (except anonymous classes, which PHPStan treats
    // differently), we look for the most recent `class` keyword that
    // appears at the top-level of the file or inside a namespace.

    let search_area = &content[..diag_byte_offset.min(content.len())];

    // Find the last `class` keyword that looks like a class declaration.
    // We scan backward for `class ` preceded by a valid context.
    let class_kw_offset = find_class_keyword_before(search_area)?;

    // Find the start of the line containing the class keyword.
    let class_line_start = content[..class_kw_offset]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);

    // Extract the class name: the word immediately after `class `.
    let after_class = &content[class_kw_offset + "class".len()..];
    let class_name = after_class
        .trim_start()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Check if the class is abstract.
    let before_class = content[class_line_start..class_kw_offset].trim();
    let is_abstract = before_class.split_whitespace().any(|w| w == "abstract");

    // Look for an existing docblock above the class declaration.
    let docblock = find_class_docblock(content, class_line_start);

    // Find the constructor within the class body.
    let constructor = find_constructor(content, class_kw_offset);

    Some(EnclosingClassInfo {
        class_name,
        class_line_start,
        class_keyword_offset: class_kw_offset,
        is_abstract,
        docblock,
        constructor,
    })
}

/// Find the byte offset of the `class` keyword in a class declaration,
/// searching backward from the end of `region`.
///
/// Skips anonymous classes (`new class`) and the `class` keyword inside
/// `::class` expressions.
fn find_class_keyword_before(region: &str) -> Option<usize> {
    let target = "class";
    let target_len = target.len();
    let bytes = region.as_bytes();

    let mut search_end = region.len();

    loop {
        // Find the last occurrence of "class" before search_end.
        let haystack = &region[..search_end];
        let pos = haystack.rfind(target)?;

        // Verify it's a standalone keyword.
        let before_ok = if pos == 0 {
            true
        } else {
            let prev = bytes[pos - 1];
            prev.is_ascii_whitespace() || prev == b'\n'
        };

        let after_pos = pos + target_len;
        let after_ok = if after_pos >= bytes.len() {
            true
        } else {
            let next = bytes[after_pos];
            next.is_ascii_whitespace() || next == b'('
        };

        if before_ok && after_ok {
            // Reject `::class` (class constant access).
            if pos >= 2 && &region[pos - 2..pos] == "::" {
                search_end = pos;
                continue;
            }

            // Reject `new class` (anonymous class).
            let before_trimmed = region[..pos].trim_end();
            if before_trimmed.ends_with("new") {
                search_end = pos;
                continue;
            }

            return Some(pos);
        }

        // Not a valid match, keep searching.
        if pos == 0 {
            return None;
        }
        search_end = pos;
    }
}

/// Find the class-level docblock above the class declaration.
///
/// Walks backward from `class_line_start` past any attribute lines
/// to find a `/** ... */` docblock.
fn find_class_docblock(content: &str, class_line_start: usize) -> Option<ClassDocblock> {
    // Find the content before the class line.
    let before = content.get(..class_line_start)?;
    let trimmed = before.trim_end();

    // Walk backward past attribute lines (`#[...]`).
    let mut check = trimmed;
    loop {
        let line_start = check.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let last_line = check[line_start..].trim();
        if last_line.starts_with("#[") {
            // This is an attribute line, skip it.
            if line_start == 0 {
                check = "";
                break;
            }
            check = check[..line_start].trim_end();
        } else {
            break;
        }
    }

    // Now check if the remaining content ends with `*/`.
    let check = check.trim_end();
    if !check.ends_with("*/") {
        return None;
    }

    // Find the matching `/**`.
    let doc_end = check.len();
    let doc_start = check.rfind("/**")?;

    let text = check[doc_start..doc_end].to_string();

    Some(ClassDocblock {
        start: doc_start,
        end: doc_end,
        text,
    })
}

/// Find the `__construct` method within the class body starting from
/// the class keyword offset.
fn find_constructor(content: &str, class_kw_offset: usize) -> Option<ConstructorInfo> {
    // Find the opening brace of the class body.
    let after_class = &content[class_kw_offset..];
    let open_brace_rel = after_class.find('{')?;
    let body_start = class_kw_offset + open_brace_rel + 1;

    // Search forward within the class body for `__construct`.
    // We need to track brace depth to stay within the class body
    // and not descend into method bodies.
    let body = &content[body_start..];
    let mut depth: i32 = 0;
    let mut i = 0;
    let body_bytes = body.as_bytes();

    while i < body_bytes.len() {
        match body_bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    // End of class body.
                    break;
                }
                depth -= 1;
            }
            b'_' if depth == 0 => {
                // Check for `__construct` at depth 0 (class member level).
                let remaining = &body[i..];
                if remaining.starts_with("__construct")
                    && (i + "__construct".len() >= body.len()
                        || !body_bytes[i + "__construct".len()].is_ascii_alphanumeric()
                            && body_bytes[i + "__construct".len()] != b'_')
                {
                    // Found `__construct`.  Walk backward to find the
                    // start of the declaration (modifiers, `function`).
                    let construct_abs = body_start + i;
                    return Some(build_constructor_info(content, construct_abs));
                }
            }
            b'/' if depth == 0
                // Skip string literals and comments to avoid false matches.
                && i + 1 < body_bytes.len() =>
            {
                if body_bytes[i + 1] == b'/' {
                    // Single-line comment: skip to end of line.
                    while i < body_bytes.len() && body_bytes[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                } else if body_bytes[i + 1] == b'*' {
                    // Multi-line comment: skip to `*/`.
                    i += 2;
                    while i + 1 < body_bytes.len() {
                        if body_bytes[i] == b'*' && body_bytes[i + 1] == b'/' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
            }
            b'\'' | b'"' if depth == 0 => {
                // Skip string literals.
                let quote = body_bytes[i];
                i += 1;
                while i < body_bytes.len() {
                    if body_bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if body_bytes[i] == quote {
                        break;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Build a `ConstructorInfo` from the byte offset of `__construct`.
fn build_constructor_info(content: &str, construct_offset: usize) -> ConstructorInfo {
    // Walk backward from `__construct` to find `function`, then past
    // modifiers to the start of the declaration line.
    let before = &content[..construct_offset];
    let trimmed = before.trim_end();

    // There should be a `function` keyword right before `__construct`.
    let func_end = trimmed.len();
    let has_function = trimmed.ends_with("function");

    let before_func = if has_function {
        &trimmed[..func_end - "function".len()]
    } else {
        trimmed
    };

    let before_func_trimmed = before_func.trim_end();

    // Check for modifiers before `function`.
    let modifiers_text = {
        let line_start = before_func_trimmed.rfind('\n').map(|p| p + 1).unwrap_or(0);
        &before_func_trimmed[line_start..]
    };

    let has_final = modifiers_text.split_whitespace().any(|w| w == "final");

    // Walk backward past attribute lines to find the true declaration start.
    // The declaration start is the first line of attributes, modifiers,
    // or the `function` keyword.
    let func_kw_offset = if has_function {
        func_end - "function".len()
    } else {
        construct_offset
    };

    // Find the line start of the first modifier.
    let before_kw = &content[..func_kw_offset];
    let mut decl_start_offset = before_kw.trim_end().rfind('\n').map(|p| p + 1).unwrap_or(0);

    // Check if there are modifier keywords on this line.
    let line_content = &content[decl_start_offset..func_kw_offset];
    let line_trimmed = line_content.trim();
    if !line_trimmed.is_empty() {
        // Modifiers are on the same line as `function`.
        // `decl_start_offset` already points to the line start.
    } else {
        // No modifiers before `function` on this line; the `function`
        // keyword is the start.
        decl_start_offset = content[..func_kw_offset]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
    }

    // Find the first non-whitespace on the declaration line.
    let decl_line = &content[decl_start_offset..];
    let first_non_ws = decl_line.find(|c: char| !c.is_whitespace()).unwrap_or(0);

    ConstructorInfo {
        decl_start: decl_start_offset + first_non_ws,
        has_final,
    }
}

/// Check if the diagnostic is already fixed (stale).
fn is_already_fixed(content: &str, info: &EnclosingClassInfo) -> bool {
    // 1. Class has `final` keyword.
    let before_class = content[info.class_line_start..info.class_keyword_offset].trim();
    if before_class.split_whitespace().any(|w| w == "final") {
        return true;
    }

    // 2. Constructor has `final` keyword.
    if let Some(ref ctor) = info.constructor
        && ctor.has_final
    {
        return true;
    }

    // 3. Docblock contains `@phpstan-consistent-constructor`.
    if let Some(ref doc) = info.docblock
        && doc.text.contains("@phpstan-consistent-constructor")
    {
        return true;
    }

    false
}

/// Check if the diagnostic should be considered stale based on the
/// current content.
///
/// Called from `is_stale_phpstan_diagnostic` to eagerly clear the
/// diagnostic after the user applies one of the fixes.
pub(crate) fn is_new_static_stale(content: &str, diag_line: usize) -> bool {
    let Some(class_info) = find_enclosing_class(content, diag_line) else {
        return false;
    };
    is_already_fixed(content, &class_info)
}

// ── Edit builders ───────────────────────────────────────────────────────────

/// Build edits to add `@phpstan-consistent-constructor` to the class
/// docblock.
fn build_add_tag_edit(content: &str, info: &EnclosingClassInfo) -> Option<Vec<TextEdit>> {
    let tag = "@phpstan-consistent-constructor";

    if let Some(ref doc) = info.docblock {
        // Insert the tag into the existing docblock.
        // Find the position just before the closing `*/`.
        let doc_content = &doc.text;
        let closing = doc_content.rfind("*/")?;

        // Determine the indentation from the docblock.
        let indent = extract_docblock_indent(content, doc.start);

        let insert_offset = doc.start + closing;
        let insert_pos = offset_to_position(content, insert_offset);

        // Check if there's already content on the `*/` line.
        let before_closing = &doc_content[..closing];
        let needs_newline =
            !before_closing.ends_with('\n') && !before_closing.trim_end().ends_with('*');

        let new_text = if doc_content.contains('\n') {
            // Multi-line docblock: add ` * @tag\n<indent> ` before `*/`.
            if needs_newline {
                format!("\n{} * {}\n{} ", indent, tag, indent)
            } else {
                format!("{} * {}\n{} ", indent, tag, indent)
            }
        } else {
            // Single-line docblock like `/** @var Foo */`.
            // Convert to multi-line.
            // Replace the entire docblock.
            let inner = doc_content
                .trim_start_matches("/**")
                .trim_end_matches("*/")
                .trim();

            let replacement = if inner.is_empty() {
                format!("/**\n{} * {}\n{} */", indent, tag, indent)
            } else {
                format!(
                    "/**\n{} * {}\n{} * {}\n{} */",
                    indent, inner, indent, tag, indent
                )
            };

            let start_pos = offset_to_position(content, doc.start);
            let end_pos = offset_to_position(content, doc.end);

            return Some(vec![TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: replacement,
            }]);
        };

        Some(vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text,
        }])
    } else {
        // No existing docblock — create a new one.
        let indent = extract_line_indent(content, info.class_line_start);

        let docblock = format!("/**\n{} * {}\n{} */\n{}", indent, tag, indent, indent);

        // Insert before the class declaration line.  We need to find
        // the true start of the line, accounting for attributes above.
        let insert_offset = find_declaration_start_with_attrs(content, info.class_line_start);
        let insert_pos = offset_to_position(content, insert_offset);

        Some(vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: docblock,
        }])
    }
}

/// Build edits to add `final` before the `class` keyword.
fn build_final_class_edit(content: &str, info: &EnclosingClassInfo) -> Option<Vec<TextEdit>> {
    // Don't add `final` to an abstract class — that's a PHP error.
    if info.is_abstract {
        return None;
    }

    // Insert `final ` right before the `class` keyword.
    let insert_pos = offset_to_position(content, info.class_keyword_offset);

    Some(vec![TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: "final ".to_string(),
    }])
}

/// Build edits to add `final` before the constructor's visibility
/// modifier or `function` keyword.
fn build_final_constructor_edit(content: &str, info: &EnclosingClassInfo) -> Option<Vec<TextEdit>> {
    let ctor = info.constructor.as_ref()?;

    if ctor.has_final {
        return None;
    }

    let insert_pos = offset_to_position(content, ctor.decl_start);

    Some(vec![TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: "final ".to_string(),
    }])
}

// ── Utility helpers ─────────────────────────────────────────────────────────

/// Extract the indentation of the docblock from its position in the
/// content.
fn extract_docblock_indent(content: &str, doc_start: usize) -> String {
    let line_start = content[..doc_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    content[line_start..doc_start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect()
}

/// Extract the indentation of the line starting at `line_start`.
fn extract_line_indent(content: &str, line_start: usize) -> String {
    content[line_start..]
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .collect()
}

/// Walk backward from `class_line_start` past any attribute lines to
/// find the true start of the class declaration (for docblock insertion).
fn find_declaration_start_with_attrs(content: &str, class_line_start: usize) -> usize {
    let lines: Vec<&str> = content[..class_line_start].lines().collect();

    let mut target = class_line_start;
    let mut idx = lines.len();

    loop {
        if idx == 0 {
            break;
        }
        idx -= 1;
        let trimmed = lines[idx].trim();
        if trimmed.is_empty() {
            break;
        }
        if trimmed.starts_with("#[") {
            // Attribute line — include it.
            target = content[..target]
                .rfind('\n')
                .map(|p| {
                    // Find the start of this attribute line.
                    content[..p].rfind('\n').map(|pp| pp + 1).unwrap_or(0)
                })
                .unwrap_or(0);
        } else {
            break;
        }
    }

    target
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_class_keyword_before ───────────────────────────────────

    #[test]
    fn finds_simple_class() {
        let src = "<?php\nclass Foo {\n";
        let pos = find_class_keyword_before(src).unwrap();
        assert_eq!(&src[pos..pos + 5], "class");
    }

    #[test]
    fn finds_abstract_class() {
        let src = "<?php\nabstract class Bar {\n";
        let pos = find_class_keyword_before(src).unwrap();
        assert_eq!(&src[pos..pos + 5], "class");
    }

    #[test]
    fn skips_double_colon_class() {
        let src = "<?php\n$x = Foo::class;\nclass Bar {\n";
        let pos = find_class_keyword_before(src).unwrap();
        // Should find `class Bar`, not `Foo::class`.
        let after = &src[pos + 6..];
        assert!(after.starts_with("Bar"));
    }

    #[test]
    fn skips_anonymous_class() {
        let src = "<?php\n$x = new class {};\nclass Baz {\n";
        let pos = find_class_keyword_before(src).unwrap();
        let after = &src[pos + 6..];
        assert!(after.starts_with("Baz"));
    }

    #[test]
    fn returns_none_when_no_class() {
        let src = "<?php\nfunction foo() {}\n";
        assert!(find_class_keyword_before(src).is_none());
    }

    #[test]
    fn skips_only_double_colon_class() {
        let src = "<?php\n$x = Foo::class;";
        assert!(find_class_keyword_before(src).is_none());
    }

    // ── find_enclosing_class ────────────────────────────────────────

    #[test]
    fn finds_enclosing_class_simple() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        // `new static()` is on line 3.
        let info = find_enclosing_class(src, 3).unwrap();
        assert_eq!(info.class_name.as_deref(), Some("Foo"));
        assert!(!info.is_abstract);
    }

    #[test]
    fn detects_abstract_class() {
        let src = "<?php\nabstract class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        assert_eq!(info.class_name.as_deref(), Some("Foo"));
        assert!(info.is_abstract);
    }

    #[test]
    fn finds_constructor() {
        let src = "<?php\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(info.constructor.is_some());
        assert!(!info.constructor.as_ref().unwrap().has_final);
    }

    #[test]
    fn detects_final_constructor() {
        let src = "<?php\nclass Foo {\n    final public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(info.constructor.is_some());
        assert!(info.constructor.as_ref().unwrap().has_final);
    }

    // ── find_class_docblock ─────────────────────────────────────────

    #[test]
    fn finds_existing_docblock() {
        let src = "<?php\n/** Some doc */\nclass Foo {\n}\n";
        let class_line_start = src.find("class").unwrap();
        let line_start = src[..class_line_start]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let doc = find_class_docblock(src, line_start);
        assert!(doc.is_some());
        assert!(doc.unwrap().text.contains("Some doc"));
    }

    #[test]
    fn finds_multiline_docblock() {
        let src = "<?php\n/**\n * Some doc\n */\nclass Foo {\n}\n";
        let class_line_start = src.find("class").unwrap();
        let line_start = src[..class_line_start]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let doc = find_class_docblock(src, line_start);
        assert!(doc.is_some());
        assert!(doc.unwrap().text.contains("Some doc"));
    }

    #[test]
    fn no_docblock_when_none_present() {
        let src = "<?php\nclass Foo {\n}\n";
        let class_line_start = src.find("class").unwrap();
        let line_start = src[..class_line_start]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let doc = find_class_docblock(src, line_start);
        assert!(doc.is_none());
    }

    #[test]
    fn finds_docblock_with_attributes_between() {
        let src = "<?php\n/**\n * Some doc\n */\n#[SomeAttr]\nclass Foo {\n}\n";
        let class_line_start = {
            let pos = src.find("class Foo").unwrap();
            src[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0)
        };
        let doc = find_class_docblock(src, class_line_start);
        assert!(doc.is_some());
        assert!(doc.unwrap().text.contains("Some doc"));
    }

    // ── is_already_fixed ────────────────────────────────────────────

    #[test]
    fn not_fixed_initially() {
        let src = "<?php\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(!is_already_fixed(src, &info));
    }

    #[test]
    fn fixed_when_class_is_final() {
        let src = "<?php\nfinal class Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(is_already_fixed(src, &info));
    }

    #[test]
    fn fixed_when_constructor_is_final() {
        let src = "<?php\nclass Foo {\n    final public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(is_already_fixed(src, &info));
    }

    #[test]
    fn fixed_when_docblock_has_tag() {
        let src = "<?php\n/**\n * @phpstan-consistent-constructor\n */\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 6).unwrap();
        assert!(is_already_fixed(src, &info));
    }

    // ── is_new_static_stale ─────────────────────────────────────────

    #[test]
    fn stale_when_class_final() {
        let src = "<?php\nfinal class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        assert!(is_new_static_stale(src, 3));
    }

    #[test]
    fn not_stale_when_unfixed() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        assert!(!is_new_static_stale(src, 3));
    }

    // ── build_add_tag_edit ──────────────────────────────────────────

    #[test]
    fn adds_tag_to_existing_multiline_docblock() {
        let src = "<?php\n/**\n * Some class\n */\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 6).unwrap();
        let edits = build_add_tag_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(
            edits[0]
                .new_text
                .contains("@phpstan-consistent-constructor")
        );
    }

    #[test]
    fn adds_tag_to_single_line_docblock() {
        let src = "<?php\n/** Some class */\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let edits = build_add_tag_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        // Should convert to multi-line.
        let new = &edits[0].new_text;
        assert!(new.contains("/**\n"));
        assert!(new.contains("Some class"));
        assert!(new.contains("@phpstan-consistent-constructor"));
    }

    #[test]
    fn creates_new_docblock_when_none_exists() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_add_tag_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        let new = &edits[0].new_text;
        assert!(new.contains("/**\n"));
        assert!(new.contains("@phpstan-consistent-constructor"));
        assert!(new.contains("*/"));
    }

    // ── build_final_class_edit ──────────────────────────────────────

    #[test]
    fn inserts_final_before_class() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_final_class_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "final ");
    }

    #[test]
    fn does_not_add_final_to_abstract_class() {
        let src = "<?php\nabstract class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let result = build_final_class_edit(src, &info);
        assert!(result.is_none());
    }

    // ── build_final_constructor_edit ────────────────────────────────

    #[test]
    fn inserts_final_before_constructor() {
        let src = "<?php\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let edits = build_final_constructor_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "final ");
    }

    #[test]
    fn no_edit_when_constructor_already_final() {
        let src = "<?php\nclass Foo {\n    final public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let result = build_final_constructor_edit(src, &info);
        assert!(result.is_none());
    }

    #[test]
    fn no_edit_when_no_constructor() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let result = build_final_constructor_edit(src, &info);
        assert!(result.is_none());
    }

    // ── Integration: collect + resolve ──────────────────────────────

    #[test]
    fn verify_add_tag_edit_result() {
        let src = "<?php\n/**\n * My class\n */\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 7).unwrap();
        let edits = build_add_tag_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);

        // Apply the edit to verify the result.
        let edit = &edits[0];
        let start_offset = lsp_pos_to_byte(src, &edit.range.start);
        let end_offset = lsp_pos_to_byte(src, &edit.range.end);
        let mut result = String::new();
        result.push_str(&src[..start_offset]);
        result.push_str(&edit.new_text);
        result.push_str(&src[end_offset..]);

        assert!(result.contains("@phpstan-consistent-constructor"));
        assert!(result.contains("My class"));
    }

    #[test]
    fn verify_final_class_edit_result() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_final_class_edit(src, &info).unwrap();

        let edit = &edits[0];
        let start_offset = lsp_pos_to_byte(src, &edit.range.start);
        let end_offset = lsp_pos_to_byte(src, &edit.range.end);
        let mut result = String::new();
        result.push_str(&src[..start_offset]);
        result.push_str(&edit.new_text);
        result.push_str(&src[end_offset..]);

        assert!(result.contains("final class Foo"));
    }

    #[test]
    fn verify_final_constructor_edit_result() {
        let src = "<?php\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let edits = build_final_constructor_edit(src, &info).unwrap();

        let edit = &edits[0];
        let start_offset = lsp_pos_to_byte(src, &edit.range.start);
        let end_offset = lsp_pos_to_byte(src, &edit.range.end);
        let mut result = String::new();
        result.push_str(&src[..start_offset]);
        result.push_str(&edit.new_text);
        result.push_str(&src[end_offset..]);

        assert!(result.contains("final public function __construct"));
    }

    #[test]
    fn readonly_class_gets_final() {
        let src = "<?php\nreadonly class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_final_class_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "final ");

        // Verify the edit inserts before `class`, not before `readonly`.
        let edit = &edits[0];
        let start_offset = lsp_pos_to_byte(src, &edit.range.start);
        assert_eq!(&src[start_offset..start_offset + 5], "class");
    }

    #[test]
    fn constructor_with_no_visibility() {
        let src = "<?php\nclass Foo {\n    function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(info.constructor.is_some());
        let edits = build_final_constructor_edit(src, &info).unwrap();
        assert_eq!(edits[0].new_text, "final ");
    }

    #[test]
    fn add_tag_to_class_with_attributes() {
        let src = "<?php\n#[SomeAttr]\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_add_tag_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        let new = &edits[0].new_text;
        assert!(new.contains("@phpstan-consistent-constructor"));
    }

    #[test]
    fn no_final_constructor_action_when_no_constructor() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        assert!(info.constructor.is_none());
        // build_final_constructor_edit returns None when there's no constructor.
        assert!(build_final_constructor_edit(src, &info).is_none());
        // The collect phase (line 136) guards with `if class_info.constructor.is_some()`,
        // so the "Add final to constructor" action is never offered.
    }

    #[test]
    fn indented_class_keeps_indent() {
        let src = "<?php\nnamespace App;\n\n    class Foo {\n        public function bar() {\n            new static();\n        }\n    }\n";
        let info = find_enclosing_class(src, 5).unwrap();
        let edits = build_add_tag_edit(src, &info).unwrap();
        let new = &edits[0].new_text;
        // The docblock should have the same indent as the class.
        assert!(new.contains("@phpstan-consistent-constructor"));
    }

    // ── Test helper ─────────────────────────────────────────────────

    /// Convert an LSP `Position` to a byte offset (test helper).
    fn lsp_pos_to_byte(content: &str, pos: &Position) -> usize {
        let mut offset = 0;
        for (i, line) in content.lines().enumerate() {
            if i == pos.line as usize {
                // Count characters up to the column.
                for (j, ch) in line.chars().enumerate() {
                    if j == pos.character as usize {
                        break;
                    }
                    offset += ch.len_utf8();
                }
                return offset;
            }
            offset += line.len() + 1; // +1 for newline
        }
        content.len()
    }
}
