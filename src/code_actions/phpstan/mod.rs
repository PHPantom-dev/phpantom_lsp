//! PHPStan code actions.
//!
//! Code actions that respond to PHPStan diagnostics. Each action parses
//! the PHPStan error message, extracts the relevant information, and
//! offers a quickfix that modifies the source code to resolve the issue.
//!
//! Currently implemented:
//!
//! - **Add `@throws`** — when PHPStan reports a
//!   `missingType.checkedException` error, offer to add a `@throws`
//!   tag to the enclosing function/method docblock and import the
//!   exception class if needed.
//! - **Remove `@throws`** — when PHPStan reports `throws.unusedType`
//!   or `throws.notThrowable`, offer to remove the offending `@throws`
//!   line from the docblock.
//! - **Add `#[Override]`** — when PHPStan reports
//!   `method.missingOverride`, offer to insert `#[\Override]` above
//!   the method declaration.
//! - **Remove `#[Override]`** — when PHPStan reports
//!   `method.override` or `property.override`, offer to remove the
//!   `#[\Override]` attribute from the declaration.
//! - **Add `#[\ReturnTypeWillChange]`** — when PHPStan reports
//!   `method.tentativeReturnType`, offer to insert the attribute
//!   above the method declaration.
//! - **Fix PHPDoc type** — when PHPStan reports `return.phpDocType`,
//!   `parameter.phpDocType`, or `property.phpDocType` (a `@return`,
//!   `@param`, or `@var` tag whose type is incompatible with the
//!   native type hint), offer to update the tag type to match the
//!   native type or remove the tag entirely.
//! - **Fix prefixed class name** — when PHPStan reports
//!   `class.prefixed` (a class name with an unnecessary leading
//!   backslash), offer to replace it with the corrected name.
//! - **Remove always-true `assert()`** — when PHPStan reports
//!   `function.alreadyNarrowedType` for an `assert()` call, offer to
//!   delete the no-op statement.
//! - **Fix return void mismatch** — when PHPStan reports
//!   `return.void` (void function returns a value) or `return.empty`
//!   (non-void function has bare return), offer to strip the return
//!   expression or change the return type to `void`.  For
//!   `return.type` (return type doesn't match actual return), offer
//!   to change the native return type or update/create a `@return`
//!   docblock tag.
//! - **Remove unused return type** — when PHPStan reports
//!   `return.unusedType` (a union or intersection member is never
//!   returned), offer to remove the unused type from both the native
//!   return type and the `@return` docblock tag.
//! - **Add iterable return type** — when PHPStan reports
//!   `missingType.iterableValue` for a return type, offer to add a
//!   `@return` tag with `<mixed>` (e.g. `@return array<mixed>`).
//! - **Remove unreachable statement** — when PHPStan reports
//!   `deadCode.unreachable`, offer to delete the dead statement.
//! - **PHPStan ignore** — when the cursor is on a line with a PHPStan
//!   error, offer to add `@phpstan-ignore <identifier>`.  When PHPStan
//!   reports an unnecessary ignore, offer to remove it.

pub(crate) mod add_iterable_type;
mod add_override;
pub(crate) mod add_return_type_will_change;
pub(crate) mod add_throws;
pub(crate) mod fix_phpdoc_type;
pub(crate) mod fix_prefixed_class;
pub(crate) mod fix_return_type;
mod ignore;
pub(crate) mod new_static;
pub(crate) mod remove_assert;
pub(crate) mod remove_override;
mod remove_throws;
pub(crate) mod remove_unreachable;
pub(crate) mod remove_unused_return_type;

use tower_lsp::lsp_types::*;

use crate::Backend;

// ── Method-insertion-point helpers ──────────────────────────────────────────
//
// Shared by `add_override` and `add_return_type_will_change`, which both
// insert an attribute line above a method declaration and need to locate
// where that declaration truly starts (walking back past any existing
// modifiers and attribute lists).

use crate::util::contains_function_keyword;

/// Information about where to insert an attribute above a method
/// declaration.
pub(super) struct InsertionPoint {
    /// The byte offset where the attribute line should be inserted.
    /// This is the start of the line containing the first token of
    /// the method declaration (attribute, modifier, or `function`).
    pub(super) insert_offset: usize,
    /// The indentation whitespace of the method declaration line.
    pub(super) indent: String,
    /// The byte offset of the start of the first attribute list (if
    /// any), or the start of the first modifier / `function` keyword.
    /// Used to check if the target attribute already exists in
    /// attribute lines above the method.
    pub(super) first_token_offset: usize,
    /// The byte offset just past the end of the last attribute list
    /// before the modifiers/function keyword. If no attributes exist,
    /// this equals `first_token_offset`.
    pub(super) attrs_end_offset: usize,
}

/// Find the insertion point for an attribute on a method whose PHPStan
/// diagnostic is on `diag_line`.
///
/// The diagnostic line from PHPStan points at the method name. We need
/// to find where the method declaration truly starts, which may be
/// several lines above if there are docblocks and existing attribute
/// lists.
///
/// We walk backward from the diagnostic line to find:
/// 1. The `function` keyword on or before the diagnostic line
/// 2. Any modifiers (`public`, `static`, etc.) before `function`
/// 3. Any attribute lists (`#[...]`) before the modifiers
///
/// The insertion point is the start of the line containing the
/// earliest attribute list, or the start of the line containing the
/// first modifier/`function` keyword if no attributes exist.
pub(super) fn find_method_insertion_point(
    content: &str,
    diag_line: usize,
) -> Option<InsertionPoint> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    // Find the `function` keyword on or near the diagnostic line.
    // PHPStan places the diagnostic on the method name line, which
    // contains `function`.  In rare cases with very long signatures
    // the diagnostic might be on a continuation line, so we search
    // backward a few lines.
    let mut func_line = None;
    let search_start = diag_line.min(lines.len().saturating_sub(1));
    for i in (search_start.saturating_sub(5)..=search_start).rev() {
        if contains_function_keyword(lines[i]) {
            func_line = Some(i);
            break;
        }
    }
    let func_line = func_line?;

    // Walk backward from the function line past modifier keywords to
    // find the first modifier line.
    let mut first_decl_line = func_line;
    let mut check_line = func_line;
    loop {
        if check_line == 0 {
            break;
        }
        let prev = check_line - 1;
        let prev_trimmed = lines[prev].trim();

        // Skip blank lines between attributes and modifiers.
        if prev_trimmed.is_empty() {
            break;
        }

        // Check for modifier keywords on the previous line.
        if is_modifier_line(prev_trimmed) {
            first_decl_line = prev;
            check_line = prev;
            continue;
        }

        // Stop: the previous line is not a modifier or attribute.
        break;
    }

    // Now walk backward from `first_decl_line` to find attribute lists.
    let mut first_attr_line = first_decl_line;
    let mut check_line = first_decl_line;
    loop {
        if check_line == 0 {
            break;
        }
        let prev = check_line - 1;
        let prev_trimmed = lines[prev].trim();

        if prev_trimmed.is_empty() {
            break;
        }

        // Check for PHP attribute syntax `#[...]`.
        if is_attribute_line(prev_trimmed) {
            first_attr_line = prev;
            check_line = prev;
            continue;
        }

        break;
    }

    // Compute the line byte offset for the first attribute (or first
    // modifier/function line if no attributes).
    let target_line = first_attr_line;
    let insert_offset = line_byte_offset(content, target_line);

    // Indentation of the method declaration (use the function keyword
    // line's indentation as the canonical one).
    let indent: String = lines[func_line]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();

    // first_token_offset is the byte offset of the start of the
    // first attribute or modifier line's content.
    let first_token_offset = insert_offset;

    // attrs_end_offset: byte offset just past the last attribute line.
    let attrs_end_offset = if first_attr_line < first_decl_line {
        line_byte_offset(content, first_decl_line)
    } else {
        first_token_offset
    };

    Some(InsertionPoint {
        insert_offset,
        indent,
        first_token_offset,
        attrs_end_offset,
    })
}

/// Check if a trimmed line consists of (or starts with) PHP modifier
/// keywords, possibly followed by `function`.
pub(super) fn is_modifier_line(trimmed: &str) -> bool {
    let modifiers = [
        "public",
        "protected",
        "private",
        "static",
        "abstract",
        "final",
        "readonly",
    ];
    // The line should start with a modifier keyword.
    modifiers.iter().any(|kw| {
        trimmed.starts_with(kw)
            && trimmed[kw.len()..].starts_with(|c: char| c.is_whitespace() || c == '\0')
    })
}

/// Check if a trimmed line is a PHP attribute line (`#[...]`).
pub(super) fn is_attribute_line(trimmed: &str) -> bool {
    trimmed.starts_with("#[")
}

/// Compute the byte offset of the start of the given line number
/// (0-based).
pub(super) fn line_byte_offset(content: &str, line: usize) -> usize {
    let mut offset = 0;
    for (i, l) in content.lines().enumerate() {
        if i == line {
            return offset;
        }
        offset += l.len() + 1; // +1 for newline
    }
    content.len()
}

/// Split a PHPStan diagnostic message into the primary message and optional tip.
///
/// `parse_phpstan_message()` in `phpstan.rs` appends the tip after a `\n`
/// separator when the PHPStan JSON includes a `"tip"` field.  This helper
/// reverses that so code actions can inspect the tip independently (e.g. to
/// extract a suggested type or attribute name).
///
/// Returns `(message, Some(tip))` when a tip is present, or
/// `(message, None)` when there is no tip.
pub(crate) fn split_phpstan_tip(message: &str) -> (&str, Option<&str>) {
    match message.split_once('\n') {
        Some((msg, tip)) => (msg, Some(tip)),
        None => (message, None),
    }
}

impl Backend {
    /// Collect all PHPStan-specific code actions.
    ///
    /// Called from [`Backend::handle_code_action`](super) to gather every
    /// PHPStan quickfix that applies at the given cursor/range.
    pub(crate) fn collect_phpstan_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        // ── PHPStan ignore / remove unnecessary ignore ──────────────
        self.collect_phpstan_ignore_actions(uri, content, params, out);

        // ── Add @throws for checked exceptions ──────────────────────
        self.collect_add_throws_actions(uri, content, params, out);

        // ── Remove invalid/unused @throws ───────────────────────────
        self.collect_remove_throws_actions(uri, content, params, out);

        // ── Add #[Override] for overriding methods ──────────────────
        self.collect_add_override_actions(uri, content, params, out);

        // ── Remove #[Override] from non-overriding members ──────────
        self.collect_remove_override_actions(uri, content, params, out);

        // ── Add #[\ReturnTypeWillChange] for tentative return types ─
        self.collect_add_return_type_will_change_actions(uri, content, params, out);

        // ── Fix unsafe `new static()` ───────────────────────────────
        self.collect_new_static_actions(uri, content, params, out);

        // ── Fix PHPDoc type mismatch (@return, @param, @var) ────────
        self.collect_fix_phpdoc_type_actions(uri, content, params, out);

        // ── Fix prefixed class name ─────────────────────────────────
        self.collect_fix_prefixed_class_actions(uri, content, params, out);

        // ── Remove always-true assert() ─────────────────────────────
        self.collect_remove_assert_actions(uri, content, params, out);

        // ── Fix return type (return.void / return.empty / return.type / missingType.return) ─
        self.collect_fix_return_type_actions(uri, content, params, out);

        // ── Remove unused return type (return.unusedType) ───────────
        self.collect_remove_unused_return_type_actions(uri, content, params, out);

        // ── Add iterable return type (missingType.iterableValue) ────
        self.collect_add_iterable_type_actions(uri, content, params, out);

        // ── Remove unreachable statement ────────────────────────────
        self.collect_remove_unreachable_actions(uri, content, params, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_message_with_tip() {
        let (msg, tip) = split_phpstan_tip("Some error.\nUse #[Override] to fix.");
        assert_eq!(msg, "Some error.");
        assert_eq!(tip, Some("Use #[Override] to fix."));
    }

    #[test]
    fn returns_none_when_no_tip() {
        let (msg, tip) = split_phpstan_tip("Some error.");
        assert_eq!(msg, "Some error.");
        assert_eq!(tip, None);
    }

    #[test]
    fn empty_tip_after_newline() {
        let (msg, tip) = split_phpstan_tip("Some error.\n");
        assert_eq!(msg, "Some error.");
        assert_eq!(tip, Some(""));
    }
}
