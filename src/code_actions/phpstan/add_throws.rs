//! "Add @throws" code action for PHPStan `missingType.checkedException`.
//!
//! When PHPStan reports that a method or function throws a checked
//! exception that is not documented in the `@throws` tag, this code
//! action offers to:
//!
//! 1. Insert a `@throws ShortName` tag into the existing docblock
//!    (or create a new docblock if none exists).
//! 2. Add a `use FQN;` import statement when the exception class is
//!    not already imported.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `missingType.checkedException` overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_add_throws_actions`) performs all validation and
//! emits a lightweight `CodeAction` with a `data` payload but no `edit`.
//! Phase 2 (`resolve_add_throws`) recomputes the workspace edit on
//! demand when the user picks the action.

use std::collections::HashMap;

use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::CodeActionData;
use crate::code_actions::make_code_action_data;
use crate::completion::use_edit::{analyze_use_block, build_use_edit, use_import_conflicts};
use crate::parser::with_parsed_program;
use crate::util::{
    byte_range_to_lsp_range, offset_to_position, ranges_overlap, strip_fqn_prefix,
    strip_trailing_modifiers,
};

/// The PHPStan identifier we match on.
const CHECKED_EXCEPTION_ID: &str = "missingType.checkedException";

impl Backend {
    /// Collect "Add @throws" code actions for PHPStan
    /// `missingType.checkedException` diagnostics.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in [`resolve_add_throws`](Self::resolve_add_throws).
    pub(crate) fn collect_add_throws_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags: Vec<Diagnostic> = {
            let cache = self.phpstan_last_diags.lock();
            cache.get(uri).cloned().unwrap_or_default()
        };

        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            if identifier != CHECKED_EXCEPTION_ID {
                continue;
            }

            // Extract the exception FQN from the message.
            let exception_fqn = match extract_exception_fqn(&diag.message) {
                Some(fqn) => fqn,
                None => continue,
            };

            let short_name = crate::util::short_name(&exception_fqn);

            // Determine what name to use in the @throws tag.  If the
            // exception is already imported (or in the same namespace),
            // use the short name.  Otherwise we'll still use the short
            // name but also add a use import.
            let already_imported = file_use_map.iter().any(|(alias, fqn)| {
                alias.eq_ignore_ascii_case(short_name) && fqn.eq_ignore_ascii_case(&exception_fqn)
            });

            let same_namespace = match &file_namespace {
                Some(ns) => {
                    let ns_prefix = format!("{}\\", ns);
                    let stripped = exception_fqn.strip_prefix(&ns_prefix);
                    // Same namespace if after stripping the prefix there's
                    // no further backslash (i.e. it's a direct child).
                    stripped.is_some_and(|rest| !rest.contains('\\'))
                }
                None => !exception_fqn.contains('\\'),
            };

            let needs_import = !already_imported && !same_namespace;

            // Check for import conflicts.
            if needs_import && use_import_conflicts(&exception_fqn, &file_use_map) {
                continue;
            }

            // Find the enclosing function/method and its docblock.
            let diag_line = diag.range.start.line as usize;
            let docblock_info = match find_enclosing_docblock(content, diag_line) {
                Some(info) => info,
                None => continue,
            };

            // Check if this exception is already in @throws.
            if docblock_already_has_throws(
                &docblock_info,
                &exception_fqn,
                &file_use_map,
                &file_namespace,
            ) {
                continue;
            }

            // ── Phase 1: emit lightweight action with data ──────────
            let title = format!("Add @throws {}", short_name);

            let extra = serde_json::json!({
                "diagnostic_message": diag.message,
                "diagnostic_line": diag.range.start.line,
                "diagnostic_code": CHECKED_EXCEPTION_ID,
            });

            let data = make_code_action_data("phpstan.addThrows", uri, &params.range, extra);

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

    /// Resolve the "Add @throws" code action by computing the full
    /// workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.  Recomputes the docblock edit and (optionally)
    /// the import edit from the data payload.
    pub(crate) fn resolve_add_throws(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;

        // Parse the extra data to recover the diagnostic message.
        let diagnostic_message = data.extra.get("diagnostic_message")?.as_str()?;
        let diagnostic_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;

        // Extract the exception FQN from the message.
        let exception_fqn = extract_exception_fqn(diagnostic_message)?;
        let short_name = crate::util::short_name(&exception_fqn);

        // Look up the use_map and namespace_map for the URI.
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        // Determine if an import is needed.
        let already_imported = file_use_map.iter().any(|(alias, fqn)| {
            alias.eq_ignore_ascii_case(short_name) && fqn.eq_ignore_ascii_case(&exception_fqn)
        });

        let same_namespace = match &file_namespace {
            Some(ns) => {
                let ns_prefix = format!("{}\\", ns);
                let stripped = exception_fqn.strip_prefix(&ns_prefix);
                stripped.is_some_and(|rest| !rest.contains('\\'))
            }
            None => !exception_fqn.contains('\\'),
        };

        let needs_import = !already_imported && !same_namespace;

        // Find the enclosing docblock.
        let docblock_info = find_enclosing_docblock(content, diagnostic_line)?;

        // Build edits.
        let mut edits = Vec::new();

        // 1. Docblock edit: insert @throws tag.
        let throws_edit = build_throws_edit(content, &docblock_info, short_name);
        edits.push(throws_edit);

        // 2. Import edit (if needed).
        if needs_import {
            let use_block = analyze_use_block(content);
            if let Some(import_edits) = build_use_edit(&exception_fqn, &use_block, &file_namespace)
            {
                edits.extend(import_edits);
            }
        }

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

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract the fully-qualified exception class name from a PHPStan
/// `missingType.checkedException` message.
///
/// Expected formats:
/// - `"Method Ns\Cls::method() throws checked exception Ns\Ex but ..."`
/// - `"Function foo() throws checked exception Ns\Ex but ..."`
/// - `"Get hook for property Ns\Cls::$prop throws checked exception Ns\Ex but ..."`
pub(crate) fn extract_exception_fqn(message: &str) -> Option<String> {
    let marker = "throws checked exception ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find(" but")?;
    let fqn = rest[..end].trim();
    if fqn.is_empty() {
        return None;
    }
    // Strip leading backslash if present.
    Some(strip_fqn_prefix(fqn).to_string())
}

/// Information about an existing docblock (or the position to create one).
struct DocblockInfo {
    /// Whether a docblock already exists.
    has_docblock: bool,
    /// Byte offset of the `/**` (if exists).
    start: usize,
    /// Byte offset just past the `*/` (if exists).
    end: usize,
    /// The raw docblock text (if exists).
    text: String,
    /// Indentation whitespace of the function/method line.
    indent: String,
    /// Byte offset of the start of the function signature line
    /// (used for inserting a new docblock before it).
    sig_line_start: usize,
}

/// Find the enclosing function/method and its docblock by walking
/// backward from the given line.
///
/// We find the function signature by tracking brace depth: from the
/// diagnostic line we walk backward until we find the opening `{` at
/// depth -1 (exiting the function body).  Then we look backward past
/// modifiers to find the docblock.
fn find_enclosing_docblock(content: &str, diag_line: usize) -> Option<DocblockInfo> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    // Convert the diagnostic line to a byte offset to start searching.
    let mut diag_byte_offset = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if i == diag_line {
            break;
        }
        diag_byte_offset += line.len() + 1; // +1 for newline
    }

    let search_area = content.get(..diag_byte_offset)?;

    // Walk backward tracking brace depth to find the opening `{` of
    // the enclosing function body.
    let mut brace_depth = 0i32;
    let mut func_open_brace: Option<usize> = None;

    for (i, ch) in search_area.char_indices().rev() {
        match ch {
            '}' => brace_depth += 1,
            '{' => {
                brace_depth -= 1;
                if brace_depth < 0 {
                    func_open_brace = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let brace_pos = func_open_brace?;

    // Find the `function` keyword before the brace.
    let before_brace = content.get(..brace_pos)?;
    let mut sig_start = before_brace.len().saturating_sub(2000);
    while sig_start > 0 && !before_brace.is_char_boundary(sig_start) {
        sig_start -= 1;
    }
    let sig_region = &before_brace[sig_start..];
    let func_kw_rel = sig_region.rfind("function")?;
    let func_kw_pos = sig_start + func_kw_rel;

    // Walk backward from `function` past modifier keywords and whitespace
    // to find where the signature truly starts (for docblock detection).
    let before_func = content.get(..func_kw_pos)?;
    let trimmed = before_func.trim_end();

    // Strip trailing modifier keywords (public, protected, private, static,
    // abstract, final, readonly).
    let after_mods = strip_trailing_modifiers(trimmed);

    // Determine the byte offset of the start of the signature line
    // (the first modifier or `function` keyword).
    let sig_line_byte_start = {
        let mods_end_pos = after_mods.len();
        // The first modifier (or the function keyword itself) starts
        // right after `after_mods` (which is the content before all
        // modifiers).
        let first_token_pos = if mods_end_pos < func_kw_pos {
            // There are modifiers — find the first non-whitespace after mods.
            content[mods_end_pos..func_kw_pos]
                .find(|c: char| !c.is_whitespace())
                .map(|offset| mods_end_pos + offset)
                .unwrap_or(func_kw_pos)
        } else {
            func_kw_pos
        };
        // Walk back to the start of this token's line.
        content[..first_token_pos]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0)
    };

    let indent: String = {
        let line = &content[sig_line_byte_start..];
        line.chars()
            .take_while(|c| c.is_whitespace() && *c != '\n')
            .collect()
    };

    // Check for an existing docblock.
    let after_mods_trimmed = after_mods.trim_end();
    if after_mods_trimmed.ends_with("*/") {
        let doc_end_pos = after_mods_trimmed.len();
        if let Some(rel_open) = after_mods_trimmed.rfind("/**") {
            let doc_start_pos = rel_open;
            let text = after_mods_trimmed[doc_start_pos..doc_end_pos].to_string();
            return Some(DocblockInfo {
                has_docblock: true,
                start: doc_start_pos,
                end: doc_end_pos,
                text,
                indent,
                sig_line_start: sig_line_byte_start,
            });
        }
    }

    // No existing docblock — we'll create one.
    Some(DocblockInfo {
        has_docblock: false,
        start: 0,
        end: 0,
        text: String::new(),
        indent,
        sig_line_start: sig_line_byte_start,
    })
}

/// Check if the existing docblock already documents a `@throws` for
/// the given exception type.
///
/// `exception_fqn` is the fully-qualified name from PHPStan (no
/// leading `\`).  Each `@throws` tag in the docblock is resolved
/// through the use-map so that `@throws RuntimeException` matches
/// `App\Exceptions\RuntimeException` when the import exists.
fn docblock_already_has_throws(
    info: &DocblockInfo,
    exception_fqn: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> bool {
    if !info.has_docblock {
        return false;
    }
    let parsed = match crate::docblock::parser::parse_docblock_for_tags(&info.text) {
        Some(parsed) => parsed,
        None => return false,
    };
    // PHPStan already reports FQNs — just normalise.
    let target_fqn = exception_fqn
        .strip_prefix('\\')
        .unwrap_or(exception_fqn)
        .to_lowercase();
    for tag in parsed.tags_by_kind(mago_docblock::document::TagKind::Throws) {
        let rest = tag.description.trim();
        if let Some(type_name) = rest.split_whitespace().next() {
            let tag_fqn =
                crate::util::resolve_to_fqn(type_name, use_map, file_namespace).to_lowercase();
            if tag_fqn == target_fqn {
                return true;
            }
        }
    }
    false
}

/// Build a `TextEdit` that inserts a `@throws` tag into the docblock.
fn build_throws_edit(content: &str, info: &DocblockInfo, short_name: &str) -> TextEdit {
    if info.has_docblock {
        insert_throws_into_existing_docblock(content, info, short_name)
    } else {
        create_docblock_with_throws(content, info, short_name)
    }
}

/// Insert a `@throws` line into an existing docblock.
///
/// Strategy: insert before the closing `*/`.  If the last non-empty
/// docblock line before `*/` is not an `@throws` tag, add a blank
/// `*` separator line first (unless the docblock is a single-line
/// `/** ... */`).
fn insert_throws_into_existing_docblock(
    content: &str,
    info: &DocblockInfo,
    short_name: &str,
) -> TextEdit {
    let doc = &info.text;
    let indent = &info.indent;

    // Find the position of `*/` in the docblock.
    let close_pos = match doc.rfind("*/") {
        Some(p) => p,
        None => {
            // Shouldn't happen, but fall back to replacing the whole docblock.
            return create_docblock_with_throws(content, info, short_name);
        }
    };

    // Check if this is a single-line docblock like `/** summary */`.
    let open_to_close = &doc[3..close_pos];
    let is_single_line = !open_to_close.contains('\n');

    if is_single_line {
        // Convert to multi-line and add @throws.
        let inner = open_to_close.trim();
        let mut new_doc = format!("{}/**\n", indent);
        if !inner.is_empty() {
            new_doc.push_str(&format!("{} * {}\n", indent, inner));
            new_doc.push_str(&format!("{} *\n", indent));
        }
        new_doc.push_str(&format!("{} * @throws {}\n", indent, short_name));
        new_doc.push_str(&format!("{} */", indent));

        return TextEdit {
            range: byte_range_to_lsp_range(content, info.start, info.end),
            new_text: new_doc,
        };
    }

    // Multi-line docblock: insert before `*/`.
    // Check if we need a blank `*` separator line.
    let before_close = doc[..close_pos].trim_end();
    let last_line = before_close.lines().last().unwrap_or("");
    let last_trimmed = last_line.trim().trim_start_matches('*').trim();

    let needs_separator = !last_trimmed.is_empty()
        && !last_trimmed.starts_with("@throws")
        && last_trimmed.starts_with('@');

    // Build the text to insert before the `*/` line.
    let mut insert_text = String::new();
    if needs_separator {
        insert_text.push_str(&format!("{} *\n", indent));
    }
    insert_text.push_str(&format!("{} * @throws {}\n", indent, short_name));

    // Find where the `*/` line starts (including leading whitespace).
    let close_line_start = doc[..close_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let actual_insert_offset = info.start + close_line_start;

    // We replace from the start of the `*/` line to the same position,
    // effectively inserting before it.
    let lsp_pos = offset_to_position(content, actual_insert_offset);

    TextEdit {
        range: Range {
            start: lsp_pos,
            end: lsp_pos,
        },
        new_text: insert_text,
    }
}

/// Create a new docblock with just a `@throws` tag and insert it before
/// the function/method signature.
fn create_docblock_with_throws(_content: &str, info: &DocblockInfo, short_name: &str) -> TextEdit {
    let indent = &info.indent;
    let new_doc = format!(
        "{}/**\n{} * @throws {}\n{} */\n",
        indent, indent, short_name, indent
    );

    // Insert at the start of the signature line.
    // We need to convert sig_line_start to an LSP position.
    let lsp_pos = offset_to_position(_content, info.sig_line_start);

    TextEdit {
        range: Range {
            start: lsp_pos,
            end: lsp_pos,
        },
        new_text: new_doc,
    }
}

/// Find the line range (start, end) of the enclosing function/method body
/// for the given diagnostic line.
///
/// Returns `(opening_brace_line, closing_brace_line)` so callers can
/// check whether two diagnostics fall within the same function body.
/// This is used to batch-clear all `missingType.checkedException`
/// diagnostics for the same exception class when the user applies the
/// "Add @throws" quick fix on any one of them.
///
/// Uses the mago AST parser to find the enclosing function or method,
/// which correctly handles braces inside strings, comments, and heredocs.
pub(crate) fn find_enclosing_function_line_range(
    content: &str,
    diag_line: usize,
) -> Option<(usize, usize)> {
    // Convert the diagnostic line to a byte offset (start of line is enough).
    let mut cursor_offset = 0usize;
    let mut found = false;
    for (i, line) in content.lines().enumerate() {
        if i == diag_line {
            found = true;
            break;
        }
        cursor_offset += line.len() + 1;
    }
    if !found {
        return None;
    }
    let cursor_offset = cursor_offset as u32;

    with_parsed_program(
        content,
        "find_enclosing_function_line_range",
        |program, content| {
            find_function_range_in_statements(&program.statements, cursor_offset, content)
        },
    )
}

/// Walk top-level statements (and namespace children) looking for the
/// function or method whose body contains `cursor`.
fn find_function_range_in_statements(
    statements: &Sequence<'_, Statement<'_>>,
    cursor: u32,
    content: &str,
) -> Option<(usize, usize)> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Namespace(ns) => {
                if let Some(range) =
                    find_function_range_in_statements(ns.statements(), cursor, content)
                {
                    return Some(range);
                }
            }
            Statement::Function(func) => {
                let open = func.body.left_brace.start.offset;
                let close = func.body.right_brace.start.offset;
                if cursor >= open && cursor <= close {
                    let open_line = offset_to_position(content, open as usize).line as usize;
                    let close_line = offset_to_position(content, close as usize).line as usize;
                    return Some((open_line, close_line));
                }
            }
            Statement::Class(class) => {
                if let Some(range) =
                    find_method_range_in_members(class.members.iter(), cursor, content)
                {
                    return Some(range);
                }
            }
            Statement::Interface(iface) => {
                if let Some(range) =
                    find_method_range_in_members(iface.members.iter(), cursor, content)
                {
                    return Some(range);
                }
            }
            Statement::Trait(tr) => {
                if let Some(range) =
                    find_method_range_in_members(tr.members.iter(), cursor, content)
                {
                    return Some(range);
                }
            }
            Statement::Enum(en) => {
                if let Some(range) =
                    find_method_range_in_members(en.members.iter(), cursor, content)
                {
                    return Some(range);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check each method in a class-like's members for a concrete body
/// containing `cursor`.
fn find_method_range_in_members<'a>(
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    cursor: u32,
    content: &str,
) -> Option<(usize, usize)> {
    for member in members {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            let open = block.left_brace.start.offset;
            let close = block.right_brace.start.offset;
            if cursor >= open && cursor <= close {
                let open_line = offset_to_position(content, open as usize).line as usize;
                let close_line = offset_to_position(content, close as usize).line as usize;
                return Some((open_line, close_line));
            }
        }
    }
    None
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_exception_fqn ───────────────────────────────────────

    #[test]
    fn extracts_fqn_from_method_message() {
        let msg = "Method App\\Controllers\\Foo::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "App\\Exceptions\\BarException");
    }

    #[test]
    fn extracts_fqn_from_function_message() {
        let msg = "Function doStuff() throws checked exception App\\Exceptions\\StuffException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "App\\Exceptions\\StuffException");
    }

    #[test]
    fn extracts_fqn_from_property_hook_message() {
        let msg = "Get hook for property App\\Foo::$bar throws checked exception App\\Exceptions\\PropException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "App\\Exceptions\\PropException");
    }

    #[test]
    fn strips_leading_backslash() {
        let msg = "Method Foo::bar() throws checked exception \\Global\\SomeException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "Global\\SomeException");
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Some other PHPStan error about something.";
        assert!(extract_exception_fqn(msg).is_none());
    }

    // ── strip_trailing_modifiers ────────────────────────────────────

    #[test]
    fn strips_public_static() {
        assert_eq!(strip_trailing_modifiers("    public static").trim(), "");
    }

    #[test]
    fn strips_protected() {
        assert_eq!(
            strip_trailing_modifiers("some code\n    protected").trim(),
            "some code"
        );
    }

    #[test]
    fn does_not_strip_partial_keyword() {
        // `mypublic` should not be stripped.
        assert_eq!(strip_trailing_modifiers("mypublic"), "mypublic");
    }

    // ── docblock_already_has_throws ─────────────────────────────────

    #[test]
    fn detects_existing_throws() {
        let info = DocblockInfo {
            has_docblock: true,
            start: 0,
            end: 0,
            text: "/**\n * @throws FooException\n */".to_string(),
            indent: "    ".to_string(),
            sig_line_start: 0,
        };
        let use_map = HashMap::new();
        let ns = None;
        // PHPStan reports FQNs; global-namespace class → bare name is the FQN.
        assert!(docblock_already_has_throws(
            &info,
            "FooException",
            &use_map,
            &ns
        ));
    }

    #[test]
    fn detects_existing_throws_case_insensitive() {
        let info = DocblockInfo {
            has_docblock: true,
            start: 0,
            end: 0,
            text: "/**\n * @throws fooexception\n */".to_string(),
            indent: "    ".to_string(),
            sig_line_start: 0,
        };
        let use_map = HashMap::new();
        let ns = None;
        assert!(docblock_already_has_throws(
            &info,
            "FooException",
            &use_map,
            &ns
        ));
    }

    #[test]
    fn detects_fqn_throws() {
        let info = DocblockInfo {
            has_docblock: true,
            start: 0,
            end: 0,
            text: "/**\n * @throws \\App\\Exceptions\\FooException\n */".to_string(),
            indent: "    ".to_string(),
            sig_line_start: 0,
        };
        let mut use_map = HashMap::new();
        use_map.insert(
            "FooException".to_string(),
            "App\\Exceptions\\FooException".to_string(),
        );
        let ns = None;
        // PHPStan reports the FQN `App\Exceptions\FooException`.
        assert!(docblock_already_has_throws(
            &info,
            "App\\Exceptions\\FooException",
            &use_map,
            &ns
        ));
    }

    #[test]
    fn no_existing_throws() {
        let info = DocblockInfo {
            has_docblock: true,
            start: 0,
            end: 0,
            text: "/**\n * @param string $a\n */".to_string(),
            indent: "    ".to_string(),
            sig_line_start: 0,
        };
        let use_map = HashMap::new();
        let ns = None;
        assert!(!docblock_already_has_throws(
            &info,
            "FooException",
            &use_map,
            &ns
        ));
    }

    #[test]
    fn no_docblock_no_throws() {
        let info = DocblockInfo {
            has_docblock: false,
            start: 0,
            end: 0,
            text: String::new(),
            indent: "    ".to_string(),
            sig_line_start: 0,
        };
        let use_map = HashMap::new();
        let ns = None;
        assert!(!docblock_already_has_throws(
            &info,
            "FooException",
            &use_map,
            &ns
        ));
    }

    // ── find_enclosing_docblock ─────────────────────────────────────

    #[test]
    fn finds_existing_docblock() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        // diag_line = 6 (the throw line)
        let info = find_enclosing_docblock(php, 6).unwrap();
        assert!(info.has_docblock);
        assert!(info.text.contains("Summary."));
        assert_eq!(info.indent, "    ");
    }

    #[test]
    fn finds_no_docblock() {
        let php = "<?php\nclass Foo {\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        // diag_line = 3
        let info = find_enclosing_docblock(php, 3).unwrap();
        assert!(!info.has_docblock);
        assert_eq!(info.indent, "    ");
    }

    // ── build_throws_edit (into existing docblock) ──────────────────

    #[test]
    fn inserts_throws_into_multiline_docblock() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        let info = find_enclosing_docblock(php, 6).unwrap();
        let edit = build_throws_edit(php, &info, "RuntimeException");
        assert!(
            edit.new_text.contains("@throws RuntimeException"),
            "edit should contain @throws: {:?}",
            edit.new_text
        );
    }

    #[test]
    fn multiline_insert_does_not_double_indent_closing_tag() {
        // Simulate applying the edit: insert text at the `*/` line start.
        // The insert must NOT include trailing indent that would double up
        // with the existing `     */` line.
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        let info = find_enclosing_docblock(php, 6).unwrap();
        let edit = build_throws_edit(php, &info, "RuntimeException");

        // Apply the edit to the source text.
        let start = offset_to_position(php, 0); // unused, we apply manually
        let _ = start;
        let insert_offset = {
            let mut off = 0usize;
            for (i, line) in php.lines().enumerate() {
                if i == edit.range.start.line as usize {
                    off += edit.range.start.character as usize;
                    break;
                }
                off += line.len() + 1;
            }
            off
        };
        let end_offset = {
            let mut off = 0usize;
            for (i, line) in php.lines().enumerate() {
                if i == edit.range.end.line as usize {
                    off += edit.range.end.character as usize;
                    break;
                }
                off += line.len() + 1;
            }
            off
        };
        let mut result = String::new();
        result.push_str(&php[..insert_offset]);
        result.push_str(&edit.new_text);
        result.push_str(&php[end_offset..]);

        // The `*/` line should have exactly 4 spaces of indent (matching `/**`).
        let close_line = result.lines().find(|l| l.trim() == "*/").unwrap();
        assert_eq!(
            close_line, "     */",
            "closing */ should be aligned with the docblock (5 chars: 4 spaces + space before */).\nFull result:\n{}",
            result
        );
    }

    #[test]
    fn inserts_throws_into_single_line_docblock() {
        let php = "<?php\nclass Foo {\n    /** Summary. */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        let info = find_enclosing_docblock(php, 4).unwrap();
        let edit = build_throws_edit(php, &info, "RuntimeException");
        assert!(
            edit.new_text.contains("@throws RuntimeException"),
            "edit should contain @throws: {:?}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("Summary."),
            "edit should preserve summary: {:?}",
            edit.new_text
        );
    }

    #[test]
    fn creates_new_docblock_when_none_exists() {
        let php = "<?php\nclass Foo {\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        let info = find_enclosing_docblock(php, 3).unwrap();
        let edit = build_throws_edit(php, &info, "RuntimeException");
        assert!(
            edit.new_text.contains("/**"),
            "should create a docblock: {:?}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("@throws RuntimeException"),
            "should contain @throws: {:?}",
            edit.new_text
        );
        // Every line of the new docblock should start with the same indent
        // as the method signature (4 spaces).
        assert_eq!(
            edit.new_text, "    /**\n     * @throws RuntimeException\n     */\n",
            "new docblock should be aligned with the method"
        );
    }

    // ── Docblock with existing @throws ──────────────────────────────

    #[test]
    fn appends_after_existing_throws() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     *\n     * @throws FooException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        let info = find_enclosing_docblock(php, 8).unwrap();
        let use_map = HashMap::new();
        let ns = None;
        // PHPStan reports FQN; RuntimeException is global, so bare name is FQN.
        assert!(!docblock_already_has_throws(
            &info,
            "RuntimeException",
            &use_map,
            &ns
        ));
        let edit = build_throws_edit(php, &info, "RuntimeException");
        assert!(
            edit.new_text.contains("@throws RuntimeException"),
            "should add @throws: {:?}",
            edit.new_text
        );
    }

    // ── Docblock with @param and @return ────────────────────────────

    #[test]
    fn inserts_throws_after_return() {
        let php = "<?php\nclass Foo {\n    /**\n     * @param string $a\n     *\n     * @return string\n     */\n    public function bar(string $a): string {\n        throw new \\RuntimeException();\n    }\n}\n";
        let info = find_enclosing_docblock(php, 8).unwrap();
        let edit = build_throws_edit(php, &info, "RuntimeException");
        assert!(
            edit.new_text.contains("@throws RuntimeException"),
            "should add @throws: {:?}",
            edit.new_text
        );
    }

    #[test]
    fn inserts_throws_after_return_aligned() {
        // Reproduce the exact scenario: existing docblock with @return,
        // insert @throws.  The `*/` must not get double-indented.
        let php = concat!(
            "<?php\nclass Foo {\n",
            "    /**\n",
            "     * @return Response\n",
            "     */\n",
            "    public function clientside(): Response {\n",
            "        throw new \\RuntimeException();\n",
            "    }\n",
            "}\n",
        );
        let info = find_enclosing_docblock(php, 6).unwrap();
        let edit = build_throws_edit(php, &info, "RuntimeException");

        // Apply the edit to the source text.
        let insert_offset = {
            let mut off = 0usize;
            for (i, line) in php.lines().enumerate() {
                if i == edit.range.start.line as usize {
                    off += edit.range.start.character as usize;
                    break;
                }
                off += line.len() + 1;
            }
            off
        };
        let end_offset = {
            let mut off = 0usize;
            for (i, line) in php.lines().enumerate() {
                if i == edit.range.end.line as usize {
                    off += edit.range.end.character as usize;
                    break;
                }
                off += line.len() + 1;
            }
            off
        };
        let mut result = String::new();
        result.push_str(&php[..insert_offset]);
        result.push_str(&edit.new_text);
        result.push_str(&php[end_offset..]);

        let expected = concat!(
            "<?php\nclass Foo {\n",
            "    /**\n",
            "     * @return Response\n",
            "     *\n",
            "     * @throws RuntimeException\n",
            "     */\n",
            "    public function clientside(): Response {\n",
            "        throw new \\RuntimeException();\n",
            "    }\n",
            "}\n",
        );
        assert_eq!(
            result, expected,
            "inserted @throws must not double-indent the closing */.\nGot:\n{}",
            result
        );
    }

    // ── Standalone function ─────────────────────────────────────────

    #[test]
    fn works_with_standalone_function() {
        let php = "<?php\n/**\n * Does stuff.\n */\nfunction doStuff(): void {\n    throw new \\RuntimeException();\n}\n";
        let info = find_enclosing_docblock(php, 5).unwrap();
        assert!(info.has_docblock);
        let edit = build_throws_edit(php, &info, "RuntimeException");
        assert!(
            edit.new_text.contains("@throws RuntimeException"),
            "should add @throws: {:?}",
            edit.new_text
        );
    }

    // ── find_enclosing_function_line_range ───────────────────────────

    #[test]
    fn function_line_range_simple_method() {
        let php = concat!(
            "<?php\n",                                   // 0
            "class Foo {\n",                             // 1
            "    public function bar(): void {\n",       // 2
            "        throw new \\RuntimeException();\n", // 3
            "        throw new \\RuntimeException();\n", // 4
            "    }\n",                                   // 5
            "}\n",                                       // 6
        );
        // Diagnostic on line 3 should find body from line 2 to line 5.
        let (start, end) = find_enclosing_function_line_range(php, 3).unwrap();
        assert_eq!(start, 2, "opening brace line");
        assert_eq!(end, 5, "closing brace line");

        // Diagnostic on line 4 should find the same range.
        let (start2, end2) = find_enclosing_function_line_range(php, 4).unwrap();
        assert_eq!((start2, end2), (start, end));
    }

    #[test]
    fn function_line_range_standalone_function() {
        let php = concat!(
            "<?php\n",                               // 0
            "function doStuff(): void {\n",          // 1
            "    throw new \\RuntimeException();\n", // 2
            "}\n",                                   // 3
        );
        let (start, end) = find_enclosing_function_line_range(php, 2).unwrap();
        assert_eq!(start, 1);
        assert_eq!(end, 3);
    }

    #[test]
    fn function_line_range_nested_braces() {
        let php = concat!(
            "<?php\n",                                       // 0
            "class Foo {\n",                                 // 1
            "    public function bar(): void {\n",           // 2
            "        if (true) {\n",                         // 3
            "            throw new \\RuntimeException();\n", // 4
            "        }\n",                                   // 5
            "        throw new \\RuntimeException();\n",     // 6
            "    }\n",                                       // 7
            "}\n",                                           // 8
        );
        // Diagnostic inside the if block should still find the method body.
        let (start, end) = find_enclosing_function_line_range(php, 4).unwrap();
        assert_eq!(start, 2, "opening brace line");
        assert_eq!(end, 7, "closing brace line");

        // Diagnostic on line 6 (outside if, inside method) gives same range.
        let (start2, end2) = find_enclosing_function_line_range(php, 6).unwrap();
        assert_eq!((start2, end2), (start, end));
    }

    #[test]
    fn function_line_range_two_methods() {
        let php = concat!(
            "<?php\n",                                   // 0
            "class Foo {\n",                             // 1
            "    public function first(): void {\n",     // 2
            "        throw new \\RuntimeException();\n", // 3
            "    }\n",                                   // 4
            "    public function second(): void {\n",    // 5
            "        throw new \\RuntimeException();\n", // 6
            "    }\n",                                   // 7
            "}\n",                                       // 8
        );
        // Line 3 should be in `first()`.
        let (s1, e1) = find_enclosing_function_line_range(php, 3).unwrap();
        assert_eq!((s1, e1), (2, 4));

        // Line 6 should be in `second()`.
        let (s2, e2) = find_enclosing_function_line_range(php, 6).unwrap();
        assert_eq!((s2, e2), (5, 7));
    }

    #[test]
    fn function_line_range_returns_none_for_out_of_range() {
        let php = "<?php\necho 'hi';\n";
        assert!(find_enclosing_function_line_range(php, 99).is_none());
    }

    #[test]
    fn function_line_range_returns_none_outside_function() {
        // Line 1 is at the top level, not inside any function body.
        let php = concat!(
            "<?php\n",      // 0
            "echo 'hi';\n", // 1
        );
        assert!(find_enclosing_function_line_range(php, 1).is_none());
    }
}
