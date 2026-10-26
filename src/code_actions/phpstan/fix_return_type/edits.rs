//! `TextEdit` builders for the "Fix return type" code actions, all acting
//! on the function [`target_function`](super::target::target_function)
//! found for the diagnostic.

use tower_lsp_server::gen_lsp_types::TextEdit;

use crate::code_actions::indent_of_line_at;
use crate::php_type::PhpType;
use crate::text_position::{byte_range_to_lsp_range, line_ending};

use super::inference::InferredReturnType;
use super::target::TargetFunction;

// ── Edit builders ───────────────────────────────────────────────────────────

/// Build a `TextEdit` that fixes `return {expr};` in a void function: the
/// `return` statement of `target` the diagnostic is on.
///
/// The replacement depends on context:
///
/// - **`return null;`** → `return;` (null is not a meaningful value).
/// - **All other expressions** → `{expr};\n{indent}return;` (keep
///   the expression as a standalone statement and add a bare
///   `return;` on the next line).
///
/// When the return is the last statement before the function's closing
/// `}`, outside any loop, the bare `return;` is omitted since it would be
/// redundant.  When the return is the braceless body of an `if`, `else`,
/// loop or the like, the two statements get braces around them.
///
/// `None` for a diagnostic on an arrow function, which has no `return` to
/// remove.
pub(super) fn build_strip_return_expr_edit(
    content: &str,
    target: &TargetFunction,
) -> Option<TextEdit> {
    let statement = target.statement.as_ref()?;
    let value = statement.value?;
    let start = statement.start as usize;
    let end = statement.end? as usize;
    let range = byte_range_to_lsp_range(content, start, end);

    if statement.returns_null {
        return Some(TextEdit {
            range,
            new_text: "return;".to_string(),
        });
    }

    let expr_text = &content[value.start.offset as usize..value.end.offset as usize];
    if !statement.in_loop && is_last_statement_in_function(content, end, target.body_end? as usize)
    {
        return Some(TextEdit {
            range,
            new_text: format!("{};", expr_text),
        });
    }

    let nl = line_ending(content);
    let indent = indent_of_line_at(content, start);
    let Some(head_end) = statement.braceless_after.map(|offset| offset as usize) else {
        return Some(TextEdit {
            range,
            new_text: format!("{expr_text};{nl}{indent}return;"),
        });
    };
    // The two statements take the place of one, so the body they make up
    // needs braces.  A body on its own line gets a block in the usual
    // layout, unless a comment sits between it and the head.
    let gap = &content[head_end..start];
    if gap.contains('\n') && gap.trim().is_empty() {
        let outer = indent_of_line_at(content, head_end);
        return Some(TextEdit {
            range: byte_range_to_lsp_range(content, head_end, end),
            new_text: format!(" {{{nl}{indent}{expr_text};{nl}{indent}return;{nl}{outer}}}"),
        });
    }
    Some(TextEdit {
        range,
        new_text: format!("{{ {expr_text}; return; }}"),
    })
}

/// Build a list of `TextEdit`s that change the return type of `target` to
/// `target_type` and, when the target is `void`, remove the `@return`
/// docblock tag.
///
/// Returns `None` if the function declares no return type or it already
/// matches `target_type`.
pub(super) fn build_change_return_type_edits_to(
    content: &str,
    target: &TargetFunction,
    target_type: &PhpType,
) -> Option<Vec<TextEdit>> {
    let mut edits = vec![target.replace_return_type_edit(content, &target_type.to_string())?];

    if target_type.is_void()
        && let Some(return_tag_edit) = target.docblock.remove_return_tag_edit(content)
    {
        edits.push(return_tag_edit);
    }

    Some(edits)
}

/// The current return type declaration as read from the source text.
///
/// Combines the native type hint (`: array`) with the `@return` tag
/// type (if any) into a single effective type string that can be
/// compared against our inference result.
pub(super) struct CurrentReturnType {
    /// The native type hint after `:`, e.g. `array`, `int`.
    /// `None` when the function has no return type declaration.
    native: Option<PhpType>,
    /// The `@return` tag type, e.g. `array<int, string>`.
    /// `None` when there is no docblock or no `@return` tag.
    docblock: Option<PhpType>,
}

/// Read the current native return type and `@return` tag type of `target`.
pub(super) fn read_current_return_type(
    content: &str,
    target: &TargetFunction,
) -> CurrentReturnType {
    CurrentReturnType {
        native: target.native_return_type(content),
        docblock: target.docblock.return_type(),
    }
}

/// Decide whether to use our own inference or fall back to the
/// PHPStan tip.
///
/// Returns `true` when our inference disagrees with the current
/// declaration (native hint + `@return` tag) — meaning we can see
/// the mismatch ourselves and our type is likely more specific
/// (e.g. `list<int>` vs PHPStan's `array<int, int>`).
///
/// Returns `false` when inference failed or agrees with the
/// declaration — we can't see the bug, so the caller should trust
/// the PHPStan tip.
pub(super) fn should_use_own_inference(
    our: &Option<InferredReturnType>,
    current: &CurrentReturnType,
) -> bool {
    let Some(inferred) = our else {
        return false;
    };

    // The effective type our inference would write (prefers the rich
    // docblock type, falls back to the native hint).
    let our_effective = inferred.effective.as_ref().unwrap_or(&inferred.native);

    // The effective type currently declared (prefers the @return tag,
    // falls back to the native hint).
    let current_effective = current.docblock.as_ref().or(current.native.as_ref());
    let Some(current_effective) = current_effective else {
        return true;
    };
    !our_effective.equivalent(current_effective)
}

/// Build edits using pre-split native and effective types from our
/// own inference (where native is already a valid PHP type hint).
///
/// This is the counterpart of [`build_update_return_type_edits`] for
/// when we trust our own inference rather than the PHPStan tip.  The
/// difference is that the caller provides the native/effective split
/// directly (e.g. native=`array`, effective=`list<int>`) instead of
/// a single type string that gets split via `PhpType::to_native_hint`.
pub(super) fn build_update_return_type_edits_split(
    content: &str,
    target: &TargetFunction,
    native_type: &PhpType,
    effective_type: Option<&PhpType>,
) -> Option<Vec<TextEdit>> {
    // The effective type is the full type for the @return tag.
    // If there is no effective type, the native type is used for both
    // and no docblock is needed.
    let native_str = native_type.to_string();
    let full_type = effective_type
        .map(|e| e.to_string())
        .unwrap_or_else(|| native_str.clone());

    let mut edits = Vec::new();
    edits.extend(target.replace_return_type_edit(content, &native_str));
    // Only create a @return tag when the effective type differs from the
    // native type.
    edits.extend(set_return_tag_edit(
        content,
        target,
        &full_type,
        effective_type.is_some(),
    ));

    (!edits.is_empty()).then_some(edits)
}

/// Build a list of `TextEdit`s that update both the native return type
/// hint and the `@return` docblock tag for a `return.type` diagnostic.
///
/// The native type is changed to the base type (generics stripped) so
/// that it remains valid PHP.  The `@return` tag gets the full type
/// including any generic parameters.
///
/// When the actual type has no generics and there is no existing
/// `@return` tag, only the native type is changed.
///
/// Returns `None` if there is nothing to change.
pub(super) fn build_update_return_type_edits(
    content: &str,
    target: &TargetFunction,
    actual_type: &PhpType,
) -> Option<Vec<TextEdit>> {
    let actual_str = actual_type.to_string();
    let base_type = actual_type
        .to_native_hint()
        .unwrap_or_else(|| actual_str.clone());
    let has_generics = base_type != actual_str;

    let mut edits = Vec::new();
    edits.extend(target.replace_return_type_edit(content, &base_type));
    // Only create a @return tag when the actual type has generics —
    // otherwise the native type hint is sufficient.
    edits.extend(set_return_tag_edit(
        content,
        target,
        &actual_str,
        has_generics,
    ));

    (!edits.is_empty()).then_some(edits)
}

/// The edit that sets the `@return` tag of `target` to `return_type`:
/// rewriting the type of the tag it has, or adding one when `add` asks for
/// it.  A closure or arrow function never gets a new tag, since PHPStan
/// reads no docblock on one.
fn set_return_tag_edit(
    content: &str,
    target: &TargetFunction,
    return_type: &str,
    add: bool,
) -> Option<TextEdit> {
    target
        .docblock
        .replace_return_type_edit(content, return_type)
        .or_else(|| {
            (add && !target.anonymous)
                .then(|| target.docblock.add_return_tag_edit(content, return_type))
        })
}

/// Check whether the byte position `after_semi` (just past a `;`) is
/// the last statement in the function body that `body_end` closes.
///
/// If only `}` characters (closing nested blocks like `if`/`foreach`/
/// `try`) and whitespace/comments appear between the `;` and the
/// function's own closing `}`, then the statement is the last one in the
/// function and a trailing `return;` would be redundant.
///
/// Returns `false` when any other statement or token appears, meaning
/// the `return;` is needed to exit early.
fn is_last_statement_in_function(content: &str, after_semi: usize, body_end: usize) -> bool {
    let bytes = &content.as_bytes()[..body_end];
    let mut i = after_semi;

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // Line comment — skip to end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // Block comment — skip to `*/`.
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            b'}' => {
                // Closing brace — of an `if`/`foreach`/etc. block or of
                // the function itself.
                i += 1;
            }
            _ => return false,
        }
    }
    // Reached the function's closing brace with only `}` and whitespace —
    // the statement was the last one.
    true
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "edits_tests.rs"]
mod tests;
