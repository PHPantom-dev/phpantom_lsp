//! "Add iterable return type" code action for PHPStan `missingType.iterableValue`.
//!
//! When PHPStan reports that a method or function return type has no
//! value type specified in an iterable type (e.g. `array`, `iterable`,
//! `Traversable`), this code action offers to add a `@return` docblock
//! tag with the element type inferred from the function body.
//!
//! The element type is determined by scanning `return` statements for
//! array literals, variable types, and `new ClassName()` expressions
//! using the same resolution pipeline as `missingType.return`.  When
//! inference cannot determine a concrete type, the action falls back
//! to `<mixed>` as recommended by PHPStan's documentation.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `missingType.iterableValue` whose message mentions "return type".
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_add_iterable_type_actions`) validates the action
//! is applicable and emits a lightweight `CodeAction` with a generic
//! title ("Add `@return` type") and no `edit`.  Phase 2
//! (`resolve_add_iterable_type`) infers the element type from the
//! function body and computes the workspace edit.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::CodeActionData;
use crate::code_actions::make_code_action_data;
use crate::php_type::PhpType;
use crate::util::ranges_overlap;

// ── Constants ───────────────────────────────────────────────────────────────

/// The PHPStan identifier we match on.
const ITERABLE_VALUE_ID: &str = "missingType.iterableValue";

/// Action kind string for the add-iterable-return-type fix.
const ACTION_KIND: &str = "phpstan.addIterableType";

// ── Message parsing ─────────────────────────────────────────────────────────

/// Extract the iterable type name from a `missingType.iterableValue`
/// diagnostic message that refers to the return type.
///
/// Message formats:
/// - `Method Foo::bar() return type has no value type specified in iterable type array.`
/// - `Function foo() return type has no value type specified in iterable type array.`
/// - Messages may also mention `iterable`, `Traversable`, `Generator`,
///   `Collection`, or any other iterable class/interface.
///
/// Returns the parsed `PhpType`, or `None` if the message does not
/// refer to a return type or does not match the expected pattern.
fn extract_iterable_return_type(message: &str) -> Option<PhpType> {
    // Only handle the "return type" variant.
    if !message.contains("return type") {
        return None;
    }

    let marker = "in iterable type ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];

    // The type name ends at `.` (end of sentence) or whitespace.
    let end = rest.find(['.', '\n']).unwrap_or(rest.len());
    let type_name = rest[..end].trim();

    if type_name.is_empty() {
        return None;
    }

    Some(PhpType::parse(type_name))
}

/// Build the `@return` type from the iterable type name and a vector
/// of inferred element type arguments.
///
/// Uses PHPStan's generic syntax: `array<string>`, `iterable<User>`,
/// `Traversable<mixed>`, `array<int, string>`, etc.
fn build_return_type(iterable_type: &str, args: Vec<PhpType>) -> PhpType {
    PhpType::Generic(iterable_type.to_owned(), args)
}

// ── Docblock helpers ────────────────────────────────────────────────────────

/// Information about the docblock above a function/method, or where
/// to insert one.
pub(crate) struct FunctionDocblock {
    /// Whether an existing docblock was found.
    pub(crate) has_docblock: bool,
    /// The lines of the existing docblock (if any).
    /// Line indices into the source content's line array.
    pub(crate) doc_start_line: usize,
    pub(crate) doc_end_line: usize,
    /// Whether the existing docblock already has a `@return` tag.
    pub(crate) has_return_tag: bool,
    /// Line index of the existing `@return` tag (if any).
    pub(crate) return_tag_line: Option<usize>,
    /// Indentation of the function signature line.
    pub(crate) indent: String,
}

/// Find the docblock above the function/method signature at `sig_line`
/// and extract information about it.
pub(crate) fn find_function_docblock(lines: &[&str], sig_line: usize) -> FunctionDocblock {
    let indent: String = lines
        .get(sig_line)
        .map(|l| l.chars().take_while(|c| c.is_whitespace()).collect())
        .unwrap_or_default();

    if sig_line == 0 {
        return FunctionDocblock {
            has_docblock: false,
            doc_start_line: 0,
            doc_end_line: 0,
            has_return_tag: false,
            return_tag_line: None,
            indent,
        };
    }

    // Walk backward from the line before the function to find the
    // docblock end.  Skip attribute lines like `#[Override]`, blank
    // lines, and PHP modifier keywords (public, protected, private,
    // static, abstract, final, readonly) that may appear between the
    // docblock and the `function` keyword.
    let mut doc_end_line = None;
    for i in (0..sig_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.ends_with("*/") {
            doc_end_line = Some(i);
            break;
        }
        // Skip attributes and blank lines between function and docblock.
        if trimmed.starts_with("#[") || trimmed.is_empty() {
            continue;
        }
        // Skip PHP modifier keywords that can appear before `function`.
        if is_php_modifier_line(trimmed) {
            continue;
        }
        // Hit non-docblock, non-attribute content — no docblock.
        break;
    }

    let doc_end_line = match doc_end_line {
        Some(l) => l,
        None => {
            return FunctionDocblock {
                has_docblock: false,
                doc_start_line: 0,
                doc_end_line: 0,
                has_return_tag: false,
                return_tag_line: None,
                indent,
            };
        }
    };

    // Find the start of the docblock.
    let mut doc_start_line = doc_end_line;
    for i in (0..=doc_end_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("/**") {
            doc_start_line = i;
            break;
        }
        if trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        break;
    }

    // Scan for existing @return tag.
    let mut has_return_tag = false;
    let mut return_tag_line = None;
    for (i, line) in lines
        .iter()
        .enumerate()
        .take(doc_end_line + 1)
        .skip(doc_start_line)
    {
        if line.contains("@return") {
            has_return_tag = true;
            return_tag_line = Some(i);
            break;
        }
    }

    FunctionDocblock {
        has_docblock: true,
        doc_start_line,
        doc_end_line,
        has_return_tag,
        return_tag_line,
        indent,
    }
}

/// Check whether a trimmed line consists entirely of PHP modifier
/// keywords (public, protected, private, static, abstract, final,
/// readonly).  These can appear on separate lines before `function`.
pub(crate) fn is_php_modifier_line(trimmed: &str) -> bool {
    const MODIFIERS: &[&str] = &[
        "public",
        "protected",
        "private",
        "static",
        "abstract",
        "final",
        "readonly",
    ];
    // Every whitespace-separated token must be a modifier keyword.
    !trimmed.is_empty() && trimmed.split_whitespace().all(|w| MODIFIERS.contains(&w))
}

/// Find the line containing the `function` keyword by walking backward
/// from a given line.
pub(crate) fn find_function_keyword_line(lines: &[&str], from_line: usize) -> Option<usize> {
    (0..=from_line)
        .rev()
        .find(|&i| lines[i].contains("function ") || lines[i].contains("function("))
}

/// Walk backward from `diag_line` to find the function declaration
/// line (the line with the `function` keyword).
///
/// Tracks brace depth to exit the function body and locate the
/// opening `{`, then searches backward for the `function` keyword.
#[cfg(test)]
fn find_enclosing_function_decl_line(lines: &[&str], diag_line: usize) -> Option<usize> {
    // First find the opening `{` by tracking brace depth backward.
    let mut depth: i32 = 0;
    let mut brace_line = None;
    for i in (0..diag_line).rev() {
        for ch in lines[i].chars() {
            match ch {
                '{' => depth -= 1,
                '}' => depth += 1,
                _ => {}
            }
        }
        if depth < 0 {
            brace_line = Some(i);
            break;
        }
    }

    let brace_line = brace_line?;
    find_function_keyword_line(lines, brace_line)
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect code actions for PHPStan `missingType.iterableValue`
    /// diagnostics on return types.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in
    /// [`resolve_add_iterable_type`](Self::resolve_add_iterable_type).
    pub(crate) fn collect_add_iterable_type_actions(
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

            if identifier != ITERABLE_VALUE_ID {
                continue;
            }

            // Only handle the "return type" variant.
            let iterable_type_parsed = match extract_iterable_return_type(&diag.message) {
                Some(t) => t,
                None => continue,
            };
            let iterable_type = iterable_type_parsed.to_string();

            let diag_line = diag.range.start.line as usize;

            // The diagnostic is on the function declaration line itself.
            // Verify there is no existing `@return` tag with a generic
            // type (which would mean this diagnostic is stale).
            let func_line = if diag_line < lines.len()
                && (lines[diag_line].contains("function ")
                    || lines[diag_line].contains("function("))
            {
                diag_line
            } else {
                // The diagnostic might be on a modifier line (public,
                // static, etc.) — search forward for the function keyword.
                let end = (diag_line + 6).min(lines.len());
                let found = lines
                    .iter()
                    .enumerate()
                    .take(end)
                    .skip(diag_line)
                    .find(|(_, line)| line.contains("function ") || line.contains("function("));
                match found {
                    Some((i, _)) => i,
                    None => continue,
                }
            };

            let docblock = find_function_docblock(&lines, func_line);

            // Skip if there is already a @return tag with a generic type.
            if docblock.has_return_tag
                && let Some(ret_line) = docblock.return_tag_line
            {
                let ret_text = lines[ret_line];
                if docblock_return_line_has_type_structure(ret_text) {
                    continue;
                }
            }

            // Use a generic title — type inference is deferred to Phase 2.
            let title = "Add @return type".to_string();

            let extra = serde_json::json!({
                "diagnostic_line": diag_line,
                "iterable_type": &iterable_type,
                "func_line": func_line,
            });

            let data = make_code_action_data(ACTION_KIND, uri, &params.range, extra);

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

    /// Resolve the "Add iterable return type" code action by computing
    /// the full workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.  Infers the element type from the function
    /// body using the same resolution pipeline as `missingType.return`,
    /// then inserts (or updates) a `@return` tag.
    pub(crate) fn resolve_add_iterable_type(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let iterable_type = data.extra.get("iterable_type")?.as_str()?;
        let func_line = data.extra.get("func_line")?.as_u64()? as usize;

        // Infer the element type from the function body.
        let element_args = self
            .infer_iterable_element_type(&data.uri, content, func_line, iterable_type)
            .unwrap_or_else(|| vec![PhpType::mixed()]);

        let return_type = build_return_type(iterable_type, element_args).to_string();

        let lines: Vec<&str> = content.lines().collect();
        if func_line >= lines.len() {
            return None;
        }

        // Find the function signature line (might differ from func_line
        // if there are attributes between them).
        let sig_line = find_function_keyword_line(&lines, func_line).unwrap_or(func_line);
        let docblock = find_function_docblock(&lines, sig_line);

        let mut edits = Vec::new();

        if docblock.has_docblock {
            if docblock.has_return_tag {
                // Replace the existing @return tag's type.
                if let Some(ret_line_idx) = docblock.return_tag_line {
                    let ret_line = lines[ret_line_idx];
                    // Find `@return` and replace everything after it
                    // up to the end of the type.
                    if let Some(tag_pos) = ret_line.find("@return") {
                        let after_tag = &ret_line[tag_pos + "@return".len()..];
                        let type_start = after_tag
                            .find(|c: char| !c.is_whitespace())
                            .unwrap_or(after_tag.len());
                        let type_text = &after_tag[type_start..];

                        // The existing type ends at the next whitespace
                        // or end of line.
                        let type_end = type_text
                            .find(|c: char| c.is_whitespace())
                            .unwrap_or(type_text.len());

                        let abs_start = tag_pos + "@return".len() + type_start;
                        let abs_end = abs_start + type_end;

                        edits.push(TextEdit {
                            range: Range {
                                start: Position::new(ret_line_idx as u32, abs_start as u32),
                                end: Position::new(ret_line_idx as u32, abs_end as u32),
                            },
                            new_text: return_type,
                        });
                    }
                }
            } else {
                // Insert a new @return tag into the existing docblock.
                // Insert before the closing `*/`.
                let doc_end = docblock.doc_end_line;
                let close_line = lines[doc_end];

                // Check if this is a single-line docblock `/** ... */`.
                if docblock.doc_start_line == doc_end {
                    // Convert single-line to multi-line.
                    let trimmed = close_line.trim();
                    let inner = trimmed
                        .strip_prefix("/**")
                        .and_then(|s| s.strip_suffix("*/"))
                        .map(|s| s.trim())
                        .unwrap_or("");

                    let indent = &docblock.indent;
                    let mut new_doc = format!("{}/**\n", indent);
                    if !inner.is_empty() {
                        new_doc.push_str(&format!("{} * {}\n", indent, inner));
                        new_doc.push_str(&format!("{} *\n", indent));
                    }
                    new_doc.push_str(&format!("{} * @return {}\n", indent, return_type));
                    new_doc.push_str(&format!("{} */", indent));

                    edits.push(TextEdit {
                        range: Range {
                            start: Position::new(doc_end as u32, 0),
                            end: Position::new(doc_end as u32, close_line.len() as u32),
                        },
                        new_text: new_doc,
                    });
                } else {
                    // Multi-line docblock: insert @return before `*/`.
                    let indent = &docblock.indent;

                    // Check the last content line before `*/` to decide
                    // whether to add a separator.
                    let prev_line = if doc_end > docblock.doc_start_line {
                        lines[doc_end - 1].trim()
                    } else {
                        ""
                    };
                    let prev_trimmed = prev_line.trim_start_matches('*').trim();
                    let needs_separator = !prev_trimmed.is_empty()
                        && !prev_trimmed.starts_with("@return")
                        && !prev_trimmed.starts_with("@throws")
                        && prev_trimmed.starts_with('@');

                    let mut insert_text = String::new();
                    if needs_separator {
                        insert_text.push_str(&format!("{} *\n", indent));
                    }
                    insert_text.push_str(&format!("{} * @return {}\n", indent, return_type));

                    // Insert before the `*/` line.
                    edits.push(TextEdit {
                        range: Range {
                            start: Position::new(doc_end as u32, 0),
                            end: Position::new(doc_end as u32, 0),
                        },
                        new_text: insert_text,
                    });
                }
            }
        } else {
            // No existing docblock — create one with a @return tag.
            let indent = &docblock.indent;
            let new_doc = format!(
                "{}/**\n{} * @return {}\n{} */\n",
                indent, indent, return_type, indent
            );

            // Insert before the function signature line.
            edits.push(TextEdit {
                range: Range {
                    start: Position::new(sig_line as u32, 0),
                    end: Position::new(sig_line as u32, 0),
                },
                new_text: new_doc,
            });
        }

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// Infer the element type of an iterable return by scanning the
    /// function body.
    ///
    /// Delegates to [`infer_return_type_for_function`](Self::infer_return_type_for_function)
    /// in `fix_return_type.rs` to get the effective return type.
    /// When the inferred type is a generic iterable (e.g. `list<string>`,
    /// `array<int, User>`), the inner type parameter(s) are extracted
    /// and returned as a `Vec<PhpType>`.
    /// When it's a bare `array` or `mixed`, returns `None` so the caller
    /// falls back to `vec![PhpType::mixed()]`.
    fn infer_iterable_element_type(
        &self,
        uri: &str,
        content: &str,
        func_line: usize,
        iterable_type: &str,
    ) -> Option<Vec<PhpType>> {
        let inferred = self.infer_return_type_for_function(uri, content, func_line, false)?;

        // Prefer the effective type (richer, e.g. `list<string>`),
        // falling back to the native type.
        let parsed = inferred
            .effective
            .as_ref()
            .unwrap_or(&inferred.native)
            .clone();

        // If the inferred type is just `array`, `mixed`, or the bare
        // iterable type, we can't determine element types.
        if parsed.is_bare_array() || parsed.is_mixed() || parsed.is_named_ci(iterable_type) {
            return None;
        }

        // Try to extract the generic parameter(s) from the inferred type.
        // e.g. `list<string>` → [string], `array<int, User>` → [int, User]
        if let PhpType::Generic(_, args) = &parsed {
            if !args.is_empty() && args.iter().all(|a| !a.is_mixed()) {
                return Some(args.clone());
            }
            return None;
        }

        // The inferred type is a concrete non-iterable type (e.g.
        // `string` when the function returns `['a', 'b']` and the
        // resolver collapsed it).  This shouldn't normally happen for
        // an iterable return, but use it as the element type.
        if !parsed.is_void() && !parsed.is_null() {
            return Some(vec![parsed]);
        }

        None
    }
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a `missingType.iterableValue` return-type diagnostic
/// is stale.
///
/// The diagnostic is stale when:
/// - The function/method now has a `@return` tag containing a generic
///   type (indicated by `<` or `[]` in the tag).
/// - The diagnostic line no longer exists.
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/mod.rs`.
pub(crate) fn is_add_iterable_type_stale(content: &str, diag_line: usize, message: &str) -> bool {
    // Only handle the return-type variant.
    if !message.contains("return type") {
        return false;
    }

    let lines: Vec<&str> = content.lines().collect();

    if diag_line >= lines.len() {
        return true; // line doesn't exist any more → stale
    }

    // Find the function keyword line (the diagnostic is on the
    // function declaration or a modifier line).
    let func_line =
        if lines[diag_line].contains("function ") || lines[diag_line].contains("function(") {
            diag_line
        } else {
            // Search forward for the function keyword.
            let end = (diag_line + 6).min(lines.len());
            let found = lines
                .iter()
                .enumerate()
                .take(end)
                .skip(diag_line)
                .find(|(_, line)| line.contains("function ") || line.contains("function("));
            match found {
                Some((l, _)) => l,
                None => return false,
            }
        };

    let docblock = find_function_docblock(&lines, func_line);

    if !docblock.has_return_tag {
        return false;
    }

    // Check if the @return tag has a generic type.
    if let Some(ret_line_idx) = docblock.return_tag_line {
        let ret_text = lines[ret_line_idx];
        if docblock_return_line_has_type_structure(ret_text) {
            return true;
        }
    }

    false
}

/// Check whether a `@return` docblock line already contains a type with
/// generic parameters, array slice syntax, or other type structure.
///
/// Extracts the type token from the line and uses `PhpType::parse()` +
/// `has_type_structure()` for a structured check instead of a raw
/// `.contains('<')` heuristic.
fn docblock_return_line_has_type_structure(line: &str) -> bool {
    // Strip the leading ` * @return ` (or similar) prefix to get the type text.
    let trimmed = line.trim_start().trim_start_matches('*').trim_start();
    let rest = if let Some(after) = trimmed.strip_prefix("@return") {
        after.trim_start()
    } else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let (type_token, _) = crate::docblock::type_strings::split_type_token(rest);
    if type_token.is_empty() {
        return false;
    }
    let parsed = crate::php_type::PhpType::parse(type_token);
    parsed.has_type_structure()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_iterable_return_type ────────────────────────────────

    #[test]
    fn extracts_array_from_method_message() {
        let msg =
            "Method Foo::bar() return type has no value type specified in iterable type array.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("array"))
        );
    }

    #[test]
    fn extracts_array_from_function_message() {
        let msg = "Function foo() return type has no value type specified in iterable type array.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("array"))
        );
    }

    #[test]
    fn extracts_iterable_type() {
        let msg =
            "Method Foo::bar() return type has no value type specified in iterable type iterable.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("iterable"))
        );
    }

    #[test]
    fn extracts_traversable_type() {
        let msg = "Method Foo::bar() return type has no value type specified in iterable type Traversable.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("Traversable"))
        );
    }

    #[test]
    fn extracts_generator_type() {
        let msg =
            "Function gen() return type has no value type specified in iterable type Generator.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("Generator"))
        );
    }

    #[test]
    fn extracts_collection_type() {
        let msg = "Method Foo::bar() return type has no value type specified in iterable type Collection.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("Collection"))
        );
    }

    #[test]
    fn ignores_parameter_variant() {
        let msg = "Method Foo::bar() has parameter $items with no value type specified in iterable type array.";
        assert_eq!(extract_iterable_return_type(msg), None);
    }

    #[test]
    fn ignores_property_variant() {
        let msg = "Property Foo::$items type has no value type specified in iterable type array.";
        assert_eq!(extract_iterable_return_type(msg), None);
    }

    #[test]
    fn ignores_unrelated_message() {
        let msg = "Method Foo::bar() should return int but returns string.";
        assert_eq!(extract_iterable_return_type(msg), None);
    }

    // ── build_return_type ──────────────────────────────────────────

    #[test]
    fn builds_array_with_element_type() {
        assert_eq!(
            build_return_type("array", vec![PhpType::parse("string")]).to_string(),
            "array<string>"
        );
    }

    #[test]
    fn builds_iterable_mixed() {
        assert_eq!(
            build_return_type("iterable", vec![PhpType::parse("mixed")]).to_string(),
            "iterable<mixed>"
        );
    }

    #[test]
    fn builds_traversable_with_element() {
        assert_eq!(
            build_return_type("Traversable", vec![PhpType::parse("User")]).to_string(),
            "Traversable<User>"
        );
    }

    #[test]
    fn builds_array_with_key_value() {
        assert_eq!(
            build_return_type(
                "array",
                vec![PhpType::parse("int"), PhpType::parse("string")]
            )
            .to_string(),
            "array<int, string>"
        );
    }

    // ── find_function_docblock ─────────────────────────────────────

    #[test]
    fn finds_existing_multiline_docblock() {
        let src =
            "    /**\n     * Summary.\n     */\n    public function foo(): array\n    {\n    }";
        let lines: Vec<&str> = src.lines().collect();
        let db = find_function_docblock(&lines, 3);
        assert!(db.has_docblock);
        assert_eq!(db.doc_start_line, 0);
        assert_eq!(db.doc_end_line, 2);
        assert!(!db.has_return_tag);
        assert_eq!(db.indent, "    ");
    }

    #[test]
    fn finds_docblock_with_return_tag() {
        let src = "    /**\n     * Summary.\n     * @return array\n     */\n    public function foo(): array\n    {\n    }";
        let lines: Vec<&str> = src.lines().collect();
        let db = find_function_docblock(&lines, 4);
        assert!(db.has_docblock);
        assert!(db.has_return_tag);
        assert_eq!(db.return_tag_line, Some(2));
    }

    #[test]
    fn no_docblock() {
        let src = "    public function foo(): array\n    {\n    }";
        let lines: Vec<&str> = src.lines().collect();
        let db = find_function_docblock(&lines, 0);
        assert!(!db.has_docblock);
        assert_eq!(db.indent, "    ");
    }

    #[test]
    fn finds_single_line_docblock() {
        let src = "    /** Summary. */\n    public function foo(): array\n    {\n    }";
        let lines: Vec<&str> = src.lines().collect();
        let db = find_function_docblock(&lines, 1);
        assert!(db.has_docblock);
        assert_eq!(db.doc_start_line, 0);
        assert_eq!(db.doc_end_line, 0);
        assert!(!db.has_return_tag);
    }

    #[test]
    fn skips_attributes_between_docblock_and_function() {
        let src = "    /**\n     * Summary.\n     */\n    #[Override]\n    public function foo(): array\n    {\n    }";
        let lines: Vec<&str> = src.lines().collect();
        let db = find_function_docblock(&lines, 4);
        assert!(db.has_docblock);
        assert_eq!(db.doc_start_line, 0);
        assert_eq!(db.doc_end_line, 2);
    }

    // ── Stale detection ────────────────────────────────────────────

    #[test]
    fn stale_when_return_tag_has_generic_type() {
        let src = "    /**\n     * @return array<string>\n     */\n    public function foo(): array\n    {\n    }";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(is_add_iterable_type_stale(src, 3, msg));
    }

    #[test]
    fn stale_when_return_tag_has_array_suffix() {
        let src = "    /**\n     * @return string[]\n     */\n    public function foo(): array\n    {\n    }";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(is_add_iterable_type_stale(src, 3, msg));
    }

    #[test]
    fn not_stale_when_no_return_tag() {
        let src =
            "    /**\n     * Summary.\n     */\n    public function foo(): array\n    {\n    }";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(!is_add_iterable_type_stale(src, 3, msg));
    }

    #[test]
    fn not_stale_when_return_tag_has_no_generic() {
        let src = "    /**\n     * @return array\n     */\n    public function foo(): array\n    {\n    }";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(!is_add_iterable_type_stale(src, 3, msg));
    }

    #[test]
    fn not_stale_for_non_return_variant() {
        let src = "    public function foo(array $items): void\n    {\n    }";
        let msg = "Method Foo::foo() has parameter $items with no value type specified in iterable type array.";
        assert!(!is_add_iterable_type_stale(src, 0, msg));
    }

    #[test]
    fn stale_when_line_gone() {
        let src = "    public function foo(): array\n    {\n    }";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(is_add_iterable_type_stale(src, 100, msg));
    }

    #[test]
    fn stale_on_modifier_line_with_generic_return() {
        let src = "    /**\n     * @return array<int, string>\n     */\n    public\n    function foo(): array\n    {\n    }";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        // Diagnostic is on line 3 (the `public` modifier line).
        assert!(is_add_iterable_type_stale(src, 3, msg));
    }

    // ── find_enclosing_function_decl_line ───────────────────────────

    #[test]
    fn finds_function_decl_from_body() {
        let src =
            "class Foo {\n    public function bar(): array\n    {\n        return [];\n    }\n}";
        let lines: Vec<&str> = src.lines().collect();
        // diag_line = 3 (return statement)
        assert_eq!(find_enclosing_function_decl_line(&lines, 3), Some(1));
    }

    #[test]
    fn returns_none_when_no_function() {
        let src = "class Foo {\n    public $x = 1;\n}";
        let lines: Vec<&str> = src.lines().collect();
        assert_eq!(find_enclosing_function_decl_line(&lines, 1), None);
    }
}
