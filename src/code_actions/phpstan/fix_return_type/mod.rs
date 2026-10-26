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
//! - [`target`] — the function a diagnostic is about, found in the parsed
//!   file, with the positions the fixes edit.
//! - [`inference`] — return-type inference from function bodies
//!   (return-statement scanning, literal inference).
//! - [`edits`] — `TextEdit` builders for the fixes.
//! - [`message_parse`] — extraction of type names from PHPStan
//!   diagnostic messages.

mod edits;
mod inference;
mod message_parse;
mod target;

pub(crate) use inference::enrichment_return_type;
pub(in crate::code_actions::phpstan) use target::{Anchor, target_function};

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, deferred_quickfix};
use crate::php_type::PhpType;

use edits::{
    build_change_return_type_edits_to, build_strip_return_expr_edit,
    build_update_return_type_edits, build_update_return_type_edits_split, read_current_return_type,
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
        out: &mut Vec<CodeActionResponse>,
    ) {
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
            };

            let diag_line = diag.range.start.line as usize;

            match identifier {
                RETURN_VOID_ID => {
                    if !crate::util::message_text(&diag.message).ends_with(RETURN_VOID_MSG_SUFFIX) {
                        continue;
                    }
                    let Some(target) = target_function(
                        content,
                        diag_line,
                        Anchor::Return,
                        crate::util::message_text(&diag.message),
                    ) else {
                        continue;
                    };

                    // ── Fix 1: Strip return expression ──────────────
                    // An arrow function has no `return` to strip.
                    if build_strip_return_expr_edit(content, &target).is_some() {
                        let extra = serde_json::json!({
                            "identifier": RETURN_VOID_ID,
                            "message": &diag.message,
                        });

                        out.push(deferred_quickfix(
                            "Remove return statement".to_string(),
                            ACTION_KIND_STRIP_EXPR,
                            uri,
                            &params.range,
                            vec![diag.clone()],
                            false,
                            extra,
                        ));
                    }

                    // ── Fix 2: Change return type to match actual ───
                    // Extract the actual type from the message:
                    // "... returns {actual} but should not return anything."
                    // Skip when the actual type is `null` — returning null
                    // from a void function is not a type mismatch, it's
                    // just a habit.  The "Remove return statement" fix above
                    // handles it.
                    if let Some(actual_type) =
                        extract_actual_type(crate::util::message_text(&diag.message))
                        && !actual_type.is_null()
                    {
                        // Verify the change-type fix is applicable (the
                        // function has a return type that can be replaced).
                        if build_change_return_type_edits_to(content, &target, &actual_type)
                            .is_some()
                        {
                            let actual_str = actual_type.to_string();
                            let extra = serde_json::json!({
                                "identifier": RETURN_VOID_ID,
                                "message": &diag.message,
                                "actual_type": actual_str,
                            });

                            out.push(deferred_quickfix(
                                format!("Change return type to {}", actual_str),
                                ACTION_KIND_CHANGE_TYPE_TO_ACTUAL,
                                uri,
                                &params.range,
                                vec![diag.clone()],
                                true,
                                extra,
                            ));
                        }
                    }
                }
                RETURN_EMPTY_ID => {
                    if !crate::util::message_text(&diag.message).contains(RETURN_EMPTY_MSG_FRAGMENT)
                    {
                        continue;
                    }

                    // Verify the fix is applicable.
                    let applies = target_function(
                        content,
                        diag_line,
                        Anchor::Return,
                        crate::util::message_text(&diag.message),
                    )
                    .is_some_and(|target| {
                        build_change_return_type_edits_to(content, &target, &PhpType::void())
                            .is_some()
                    });
                    if !applies {
                        continue;
                    }

                    let title = "Change return type to void".to_string();

                    let extra = serde_json::json!({
                        "identifier": RETURN_EMPTY_ID,
                        "message": &diag.message,
                    });

                    out.push(deferred_quickfix(
                        title,
                        ACTION_KIND_CHANGE_TYPE,
                        uri,
                        &params.range,
                        vec![diag.clone()],
                        true,
                        extra,
                    ));
                }
                RETURN_TYPE_ID => {
                    // "Method Foo::bar() should return {expected} but returns {actual}."
                    // Just verify the message is parseable and names a
                    // function the fix can edit — defer the inference to
                    // Phase 2.
                    if extract_return_type_actual(crate::util::message_text(&diag.message))
                        .is_none()
                        || target_function(
                            content,
                            diag_line,
                            Anchor::Return,
                            crate::util::message_text(&diag.message),
                        )
                        .is_none()
                    {
                        continue;
                    }

                    let extra = serde_json::json!({
                        "identifier": RETURN_TYPE_ID,
                        "message": &diag.message,
                    });

                    out.push(deferred_quickfix(
                        "Update return type".to_string(),
                        ACTION_KIND_UPDATE_RETURN_TYPE,
                        uri,
                        &params.range,
                        vec![diag.clone()],
                        false,
                        extra,
                    ));
                }
                MISSING_TYPE_RETURN_ID => {
                    // "Method Foo::bar() has no return type specified."
                    // Defer type inference to the resolve phase — it can
                    // be expensive and the collect phase runs on every
                    // cursor move.  Just validate that the function has
                    // no return type yet, and a body to infer one from.
                    let Some(target) = target_function(
                        content,
                        diag_line,
                        Anchor::Declaration,
                        crate::util::message_text(&diag.message),
                    ) else {
                        continue;
                    };
                    if target.has_return_type() || target.body_end.is_none() {
                        continue;
                    }

                    let extra = serde_json::json!({
                        "identifier": MISSING_TYPE_RETURN_ID,
                        "message": &diag.message,
                    });

                    out.push(deferred_quickfix(
                        "Add return type".to_string(),
                        ACTION_KIND_ADD_TYPE,
                        uri,
                        &params.range,
                        vec![diag.clone()],
                        true,
                        extra,
                    ));
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
        let diag_msg = extra.get("message")?.as_str()?;
        // Every fix but the one that adds a missing return type is offered
        // for a diagnostic on a `return`.
        let anchor = if data.action_kind == ACTION_KIND_ADD_TYPE {
            Anchor::Declaration
        } else {
            Anchor::Return
        };
        let target = target_function(content, diag_line, anchor, diag_msg)?;

        let doc_uri: Uri = data.uri.parse().ok()?;

        match data.action_kind.as_str() {
            ACTION_KIND_STRIP_EXPR => {
                let edit = build_strip_return_expr_edit(content, &target)?;
                Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
            }
            ACTION_KIND_CHANGE_TYPE_TO_ACTUAL => {
                let actual_type_str = extra.get("actual_type")?.as_str()?;
                let actual_type = PhpType::parse(actual_type_str);
                let edits = build_change_return_type_edits_to(content, &target, &actual_type)?;
                Some(crate::code_actions::single_file_edit(doc_uri, edits))
            }
            ACTION_KIND_UPDATE_RETURN_TYPE => {
                let tip_type = extract_return_type_actual(diag_msg)?;

                // Run our own inference and compare to the current
                // declaration.  If they differ, our inference spotted
                // the mismatch — use it (it already has the correct
                // native/effective split, e.g. native=`array`,
                // effective=`list<int>`).  If they agree, we can't
                // see the problem ourselves — trust the PHPStan tip.
                let our = self.infer_return_type_at(&data.uri, content, target.start, false);
                let current = read_current_return_type(content, &target);

                let edits = if should_use_own_inference(&our, &current) {
                    let inferred = our?;
                    build_update_return_type_edits_split(
                        content,
                        &target,
                        &inferred.native,
                        inferred.effective.as_ref(),
                    )?
                } else {
                    // Trust the PHPStan tip.
                    build_update_return_type_edits(content, &target, &tip_type)?
                };

                Some(crate::code_actions::single_file_edit(doc_uri, edits))
            }
            ACTION_KIND_CHANGE_TYPE => {
                let void = PhpType::void();
                let edits = build_change_return_type_edits_to(content, &target, &void)?;
                Some(crate::code_actions::single_file_edit(doc_uri, edits))
            }
            ACTION_KIND_ADD_TYPE => {
                // Infer the type now (deferred from collect phase).
                let inferred =
                    self.infer_return_type_at(&data.uri, content, target.start, false)?;

                // Insert `: native_type` after the closing paren.
                let mut edits =
                    vec![target.add_return_type_edit(content, &inferred.native.to_string())];

                // When the effective type is richer than the native hint,
                // add a `@return` docblock tag.  An existing `@return` tag
                // is left alone: the user wrote it on purpose.
                if let Some(ref eff_type) = inferred.effective
                    && target.docblock.return_tag().is_none()
                {
                    edits.push(
                        target
                            .docblock
                            .add_return_tag_edit(content, &eff_type.to_string()),
                    );
                }

                Some(crate::code_actions::single_file_edit(doc_uri, edits))
            }
            _ => None,
        }
    }
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a `return.void`, `return.empty` or `missingType.return`
/// diagnostic with `message` is stale.
///
/// For `return.void`: the diagnostic is stale when the `return` on the
/// diagnostic line no longer returns anything — meaning the expression has
/// already been stripped.
///
/// For `return.empty`: the diagnostic is stale when the function's return
/// type declaration already says `void`.
///
/// For `missingType.return`: the diagnostic is stale once the function
/// declares a return type.
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/stale.rs`.
pub(crate) fn is_fix_return_type_stale(
    content: &str,
    diag_line: usize,
    identifier: &str,
    message: &str,
) -> bool {
    let Some(line) = content.lines().nth(diag_line) else {
        return true; // line doesn't exist any more → stale
    };

    match identifier {
        RETURN_VOID_ID => match target_function(content, diag_line, Anchor::Return, message) {
            // An arrow function returns its expression for as long as it
            // has one, so only a type change makes its diagnostic stale.
            Some(target) => target
                .statement
                .is_some_and(|statement| statement.value.is_none()),
            // Stale if the line no longer holds a return with an
            // expression (the user stripped it or removed the return).
            None => {
                let trimmed = line.trim();
                !trimmed.contains("return ") || trimmed == "return;"
            }
        },
        RETURN_TYPE_ID => {
            // No content heuristic — the fix might be to change the
            // code rather than the type, so we can't tell from the
            // source alone.  Cleared eagerly by codeAction/resolve.
            false
        }
        MISSING_TYPE_RETURN_ID => target_function(content, diag_line, Anchor::Declaration, message)
            .is_some_and(|target| target.has_return_type()),
        RETURN_EMPTY_ID => target_function(content, diag_line, Anchor::Return, message)
            .and_then(|target| target.native_return_type(content))
            .is_some_and(|return_type| return_type.is_void()),
        _ => false,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
