//! "Fix return type" code actions for PHPStan return-type diagnostics.
//!
//! Handles four PHPStan identifiers:
//!
//! - **`return.void`** — a `void` function returns an expression.
//!   Two fixes: remove the return statement (keeping the expression
//!   as a standalone statement followed by `return;`), or change the
//!   return type to match the actual value.
//! - **`return.empty`** — a non-void function has a bare `return;`.
//!   Fix: change the return type to `void` and remove `@return`.
//! - **`return.type`** — the return type doesn't match what the
//!   function actually returns.  Single fix: "Update return type"
//!   which updates the native type hint (to the base type, stripping
//!   generics) and updates or creates a `@return` docblock tag with
//!   the full type.  Not marked as preferred since the right fix
//!   might be to change the code rather than the signature.  The
//!   exact edits are deferred to Phase 2 to keep Phase 1 cheap.
//! - **`missingType.return`** — no return type specified.
//!   Fix: add a return type hint.  The type is inferred from the
//!   function body by scanning return statements for literals,
//!   variable types, and `new ClassName()` expressions.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_fix_return_type_actions`) validates that the
//! action is applicable and emits a lightweight `CodeAction` with a
//! `data` payload but no `edit`.  Phase 2 (`resolve_fix_return_type`)
//! recomputes the workspace edit on demand when the user picks the
//! action.
//!
//! ## Module layout
//!
//! - [`inference`] — return-type inference from function bodies
//!   (return-statement scanning, literal inference).
//! - [`edits`] — `TextEdit` builders for the fixes, plus the
//!   source-navigation helpers they share.
//! - [`message_parse`] — extraction of type names from PHPStan
//!   diagnostic messages.

mod edits;
mod inference;
mod message_parse;

pub(crate) use inference::enrichment_return_type;

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::add_iterable_type::{
    find_function_docblock, find_function_keyword_line as find_func_keyword_line,
};
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::php_type::PhpType;
use crate::text_position::ranges_overlap;

use edits::{
    build_change_return_type_edits_to, build_strip_return_expr_edit,
    build_update_return_type_edits, build_update_return_type_edits_split,
    find_close_paren_before_brace, find_function_open_brace_line, find_open_brace_from_declaration,
    gather_between_paren_and_brace, has_return_type_between, read_current_return_type,
    should_use_own_inference,
};
use message_parse::{
    RETURN_EMPTY_MSG_FRAGMENT, RETURN_VOID_MSG_SUFFIX, extract_actual_type,
    extract_return_type_actual,
};

// ── PHPStan identifiers ─────────────────────────────────────────────────────

/// PHPStan identifier for "void function returns a value".
const RETURN_VOID_ID: &str = "return.void";

/// PHPStan identifier for "non-void function has empty return".
const RETURN_EMPTY_ID: &str = "return.empty";

/// PHPStan identifier for "return type doesn't match actual return".
const RETURN_TYPE_ID: &str = "return.type";

/// PHPStan identifier for "no return type specified".
const MISSING_TYPE_RETURN_ID: &str = "missingType.return";

/// Action kind string for the strip-expression fix (return.void).
const ACTION_KIND_STRIP_EXPR: &str = "phpstan.fixReturnType.stripExpr";

/// Action kind string for changing the return type to match the actual
/// return value (return.void only — simple types without generics).
const ACTION_KIND_CHANGE_TYPE_TO_ACTUAL: &str = "phpstan.fixReturnType.changeTypeToActual";

/// Action kind string for the change-return-type-to-void fix (return.empty).
const ACTION_KIND_CHANGE_TYPE: &str = "phpstan.fixReturnType.changeType";

/// Action kind string for adding a missing return type hint.
const ACTION_KIND_ADD_TYPE: &str = "phpstan.fixReturnType.addType";

/// Action kind string for the unified "Update return type" fix
/// (return.type).  Updates both the native type hint and the `@return`
/// docblock tag in a single action.
const ACTION_KIND_UPDATE_RETURN_TYPE: &str = "phpstan.fixReturnType.updateReturnType";

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect code actions for PHPStan `return.void`, `return.empty`,
    /// `return.type`, and `missingType.return` diagnostics.
    pub(crate) fn collect_fix_return_type_actions(
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

            let diag_line = diag.range.start.line as usize;

            match identifier {
                RETURN_VOID_ID => {
                    if !diag.message.ends_with(RETURN_VOID_MSG_SUFFIX) {
                        continue;
                    }

                    // Verify the strip-expression fix is applicable.
                    if build_strip_return_expr_edit(content, diag_line).is_none() {
                        continue;
                    }

                    // ── Fix 1: Strip return expression ──────────────
                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": RETURN_VOID_ID,
                    });

                    out.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Remove return statement".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(false),
                        disabled: None,
                        data: Some(make_code_action_data(
                            ACTION_KIND_STRIP_EXPR,
                            uri,
                            &params.range,
                            extra,
                        )),
                    }));

                    // ── Fix 2: Change return type to match actual ───
                    // Extract the actual type from the message:
                    // "... returns {actual} but should not return anything."
                    // Skip when the actual type is `null` — returning null
                    // from a void function is not a type mismatch, it's
                    // just a habit.  The "Remove return statement" fix above
                    // handles it.
                    if let Some(actual_type) = extract_actual_type(&diag.message)
                        && !actual_type.is_null()
                    {
                        // Verify the change-type fix is applicable (the
                        // function has a return type that can be replaced).
                        if build_change_return_type_edits_to(content, diag_line, &actual_type)
                            .is_some()
                        {
                            let actual_str = actual_type.to_string();
                            let extra = serde_json::json!({
                                "diagnostic_line": diag_line,
                                "identifier": RETURN_VOID_ID,
                                "actual_type": actual_str,
                            });

                            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title: format!("Change return type to {}", actual_str),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diag.clone()]),
                                edit: None,
                                command: None,
                                is_preferred: Some(true),
                                disabled: None,
                                data: Some(make_code_action_data(
                                    ACTION_KIND_CHANGE_TYPE_TO_ACTUAL,
                                    uri,
                                    &params.range,
                                    extra,
                                )),
                            }));
                        }
                    }
                }
                RETURN_EMPTY_ID => {
                    if !diag.message.contains(RETURN_EMPTY_MSG_FRAGMENT) {
                        continue;
                    }

                    // Verify the fix is applicable.
                    if build_change_return_type_edits_to(content, diag_line, &PhpType::void())
                        .is_none()
                    {
                        continue;
                    }

                    let title = "Change return type to void".to_string();

                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": RETURN_EMPTY_ID,
                    });

                    let data =
                        make_code_action_data(ACTION_KIND_CHANGE_TYPE, uri, &params.range, extra);

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
                RETURN_TYPE_ID => {
                    // "Method Foo::bar() should return {expected} but returns {actual}."
                    // Just verify the message is parseable — defer all
                    // computation to Phase 2.
                    if extract_return_type_actual(&diag.message).is_none() {
                        continue;
                    }

                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": RETURN_TYPE_ID,
                        "message": &diag.message,
                    });

                    out.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Update return type".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(false),
                        disabled: None,
                        data: Some(make_code_action_data(
                            ACTION_KIND_UPDATE_RETURN_TYPE,
                            uri,
                            &params.range,
                            extra,
                        )),
                    }));
                }
                MISSING_TYPE_RETURN_ID => {
                    // "Method Foo::bar() has no return type specified."
                    // Defer type inference to the resolve phase — it can
                    // be expensive and the collect phase runs on every
                    // cursor move.  Just validate that the function has
                    // no return type yet.
                    let lines: Vec<&str> = content.lines().collect();

                    let brace_line = match find_open_brace_from_declaration(&lines, diag_line) {
                        Some(l) => l,
                        None => continue,
                    };
                    let (paren_line, paren_col) =
                        match find_close_paren_before_brace(&lines, brace_line) {
                            Some(p) => p,
                            None => continue,
                        };

                    // Check there is no existing return type.
                    if has_return_type_between(&lines, paren_line, paren_col, brace_line) {
                        continue;
                    }

                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": MISSING_TYPE_RETURN_ID,
                    });

                    out.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Add return type".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(true),
                        disabled: None,
                        data: Some(make_code_action_data(
                            ACTION_KIND_ADD_TYPE,
                            uri,
                            &params.range,
                            extra,
                        )),
                    }));
                }
                _ => continue,
            }
        }
    }

    /// Resolve a "Fix return type" code action by computing the full
    /// workspace edit.  Dispatches on the `action_kind` stored in the
    /// data payload.
    pub(crate) fn resolve_fix_return_type(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let extra = &data.extra;
        let diag_line = extra.get("diagnostic_line")?.as_u64()? as usize;

        let doc_uri: Url = data.uri.parse().ok()?;

        match data.action_kind.as_str() {
            ACTION_KIND_STRIP_EXPR => {
                let edit = build_strip_return_expr_edit(content, diag_line)?;
                let mut changes = HashMap::new();
                changes.insert(doc_uri, vec![edit]);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_CHANGE_TYPE_TO_ACTUAL => {
                let actual_type_str = extra.get("actual_type")?.as_str()?;
                let actual_type = PhpType::parse(actual_type_str);
                let edits = build_change_return_type_edits_to(content, diag_line, &actual_type)?;
                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_UPDATE_RETURN_TYPE => {
                let diag_msg = extra.get("message")?.as_str()?;
                let tip_type = extract_return_type_actual(diag_msg)?;

                // Run our own inference and compare to the current
                // declaration.  If they differ, our inference spotted
                // the mismatch — use it (it already has the correct
                // native/effective split, e.g. native=`array`,
                // effective=`list<int>`).  If they agree, we can't
                // see the problem ourselves — trust the PHPStan tip.
                //
                // The diagnostic line is a return statement inside the
                // body.  Walk backward to find the function declaration
                // line so the inference engine can locate the full body.
                let lines: Vec<&str> = content.lines().collect();
                let brace_line = find_function_open_brace_line(&lines, diag_line)?;
                let (paren_line, _) = find_close_paren_before_brace(&lines, brace_line)?;
                let func_line = find_func_keyword_line(&lines, paren_line)?;

                let our = self.infer_return_type_for_function(&data.uri, content, func_line, false);
                let current = read_current_return_type(content, diag_line);

                let edits = if should_use_own_inference(&our, &current) {
                    let inferred = our?;
                    build_update_return_type_edits_split(
                        content,
                        diag_line,
                        &inferred.native,
                        inferred.effective.as_ref(),
                    )?
                } else {
                    // Trust the PHPStan tip.
                    build_update_return_type_edits(content, diag_line, &tip_type)?
                };

                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_CHANGE_TYPE => {
                let void = PhpType::void();
                let edits = build_change_return_type_edits_to(content, diag_line, &void)?;
                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_ADD_TYPE => {
                // Infer the type now (deferred from collect phase).
                let inferred =
                    self.infer_return_type_for_function(&data.uri, content, diag_line, false)?;

                let native_str = inferred.native.to_string();

                let lines: Vec<&str> = content.lines().collect();
                let brace_line = find_open_brace_from_declaration(&lines, diag_line)?;
                let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

                let mut edits = Vec::new();

                // Insert `: native_type` after the closing paren.
                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(paren_line as u32, (paren_col + 1) as u32),
                        end: Position::new(paren_line as u32, (paren_col + 1) as u32),
                    },
                    new_text: format!(": {}", native_str),
                });

                // When the effective type is richer than the native hint,
                // add a `@return` docblock tag.
                if let Some(ref eff_type) = inferred.effective {
                    let eff = eff_type.to_string();
                    let func_line = find_func_keyword_line(&lines, paren_line).unwrap_or(diag_line);
                    let docblock_info = find_function_docblock(&lines, func_line);

                    if docblock_info.has_docblock {
                        if !docblock_info.has_return_tag {
                            // Insert @return into the existing docblock.
                            let doc_end = docblock_info.doc_end_line;
                            let close_line = lines[doc_end];

                            if docblock_info.doc_start_line == doc_end {
                                // Single-line docblock: convert to multi-line.
                                let trimmed = close_line.trim();
                                let inner = trimmed
                                    .strip_prefix("/**")
                                    .and_then(|s| s.strip_suffix("*/"))
                                    .map(|s| s.trim())
                                    .unwrap_or("");

                                let indent = &docblock_info.indent;
                                let mut new_doc = format!("{}/**\n", indent);
                                if !inner.is_empty() {
                                    new_doc.push_str(&format!("{} * {}\n", indent, inner));
                                    new_doc.push_str(&format!("{} *\n", indent));
                                }
                                new_doc.push_str(&format!("{} * @return {}\n", indent, eff));
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
                                let indent = &docblock_info.indent;

                                let prev_line = if doc_end > docblock_info.doc_start_line {
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
                                insert_text.push_str(&format!("{} * @return {}\n", indent, eff));

                                edits.push(TextEdit {
                                    range: Range {
                                        start: Position::new(doc_end as u32, 0),
                                        end: Position::new(doc_end as u32, 0),
                                    },
                                    new_text: insert_text,
                                });
                            }
                        }
                        // If the docblock already has a @return tag, we
                        // don't overwrite it — the user intentionally
                        // wrote it.
                    } else {
                        // No existing docblock — create one.
                        let indent = &docblock_info.indent;
                        let new_doc = format!(
                            "{}/**\n{} * @return {}\n{} */\n",
                            indent, indent, eff, indent
                        );
                        edits.push(TextEdit {
                            range: Range {
                                start: Position::new(func_line as u32, 0),
                                end: Position::new(func_line as u32, 0),
                            },
                            new_text: new_doc,
                        });
                    }
                }

                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            _ => None,
        }
    }
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a `return.void` or `return.empty` diagnostic is stale.
///
/// For `return.void`: the diagnostic is stale when the diagnostic line
/// contains `return;` (bare return, no expression) — meaning the
/// expression has already been stripped.
///
/// For `return.empty`: the diagnostic is stale when the enclosing
/// function's return type declaration already says `void`.
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/mod.rs`.
pub(crate) fn is_fix_return_type_stale(content: &str, diag_line: usize, identifier: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();

    if diag_line >= lines.len() {
        return true; // line doesn't exist any more → stale
    }

    match identifier {
        RETURN_VOID_ID => {
            // Stale if the line no longer contains a return with an
            // expression (user either stripped it or changed the type).
            let trimmed = lines[diag_line].trim();
            !trimmed.contains("return ") || trimmed == "return;"
        }
        RETURN_TYPE_ID => {
            // No content heuristic — the fix might be to change the
            // code rather than the type, so we can't tell from the
            // source alone.  Cleared eagerly by codeAction/resolve.
            false
        }
        MISSING_TYPE_RETURN_ID => {
            // The diagnostic is reported on the function declaration
            // line itself.  Stale if `)` is followed by `:` (a return
            // type has been added).  Simple text check on the line.
            let line = lines[diag_line];
            if let Some(paren_pos) = line.rfind(')') {
                line[paren_pos + 1..].contains(':')
            } else {
                false
            }
        }
        RETURN_EMPTY_ID => {
            // Stale if the enclosing function's return type is already
            // `void`.  The diagnostic is on a `return;` inside the
            // body, so search backward for the opening brace.
            let brace_line = match find_function_open_brace_line(&lines, diag_line) {
                Some(l) => l,
                None => return false,
            };
            let (paren_line, paren_col) = match find_close_paren_before_brace(&lines, brace_line) {
                Some(p) => p,
                None => return false,
            };

            // Gather text between `)` and `{` and check if the return
            // type is already `void`.
            let between = gather_between_paren_and_brace(&lines, paren_line, paren_col, brace_line);

            // Look for `: void` in the between text.
            if let Some(colon_pos) = between.find(':') {
                let after_colon = between[colon_pos + 1..].trim();
                // The type name after `:` ends at whitespace or `{`.
                let type_word = after_colon
                    .split(|c: char| c.is_whitespace() || c == '{')
                    .next()
                    .unwrap_or("");
                PhpType::parse(type_word).is_void()
            } else {
                false
            }
        }
        _ => false,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
