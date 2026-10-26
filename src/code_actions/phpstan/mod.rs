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

pub(crate) mod add_attribute;
pub(crate) mod add_iterable_type;
pub(crate) mod add_throws;
mod attributes;
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

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::CodeActionData;
use crate::text_position::ranges_overlap;

impl Backend {
    /// The cached PHPStan diagnostics for `uri` that overlap `range`, the
    /// ones a code action requested there can act on.
    ///
    /// Only those are cloned out of the cache, rather than every PHPStan
    /// diagnostic in the file once per action kind.
    pub(super) fn phpstan_diags_overlapping(&self, uri: &str, range: &Range) -> Vec<Diagnostic> {
        let cache = self.phpstan_tool.last_diags.lock();
        cache
            .get(uri)
            .map(|diags| {
                diags
                    .iter()
                    .filter(|diag| ranges_overlap(&diag.range, range))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The PHPStan identifier a diagnostic carries as its code, such as
/// `return.type`.
pub(super) fn phpstan_identifier(diag: &Diagnostic) -> Option<&str> {
    match &diag.code {
        Some(Code::String(s)) => Some(s),
        _ => None,
    }
}

/// The short name of the first member a PHPStan message names as
/// `Class::member`: `bar` in "Method App\Foo::bar() overrides …", `$baz`
/// in "Property App\Foo::$baz has …".
pub(super) fn phpstan_member_name(message: &str) -> Option<&str> {
    let after = &message[message.find("::")? + 2..];
    let end = after
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(after.len());
    (end > 0).then(|| &after[..end])
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
        out: &mut Vec<CodeActionResponse>,
    ) {
        // ── PHPStan ignore / remove unnecessary ignore ──────────────
        self.collect_phpstan_ignore_actions(uri, content, params, out);

        // ── Add @throws for checked exceptions ──────────────────────
        self.collect_add_throws_actions(uri, content, params, out);

        // ── Remove invalid/unused @throws ───────────────────────────
        self.collect_remove_throws_actions(uri, content, params, out);

        // ── Add #[Override] / #[\ReturnTypeWillChange] ──────────────
        self.collect_add_attribute_actions(uri, content, params, out);

        // ── Remove #[Override] from non-overriding members ──────────
        self.collect_remove_override_actions(uri, content, params, out);

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

    /// Expand `action.diagnostics` with sibling `missingType.checkedException`
    /// diagnostics for the same exception class within the same function body.
    ///
    /// When the user applies "Add @throws RuntimeException", PHPStan will
    /// have reported a separate diagnostic for every `throw new RuntimeException`
    /// in that method.  Adding the `@throws` tag fixes all of them, so we
    /// find those siblings in the cached diagnostics and add them to the
    /// action's diagnostic list.  The normal clearing logic then removes
    /// them all in one go.
    pub(crate) fn expand_sibling_checked_exception_diags(
        &self,
        data: &CodeActionData,
        content: &str,
        action: &mut CodeAction,
    ) {
        use crate::code_actions::phpstan::add_throws::{
            extract_exception_fqn, find_enclosing_function_line_range,
        };

        let diag_message = match data
            .extra
            .get("diagnostic_message")
            .and_then(|v| v.as_str())
        {
            Some(m) => m,
            None => return,
        };
        let exception_fqn = match extract_exception_fqn(diag_message) {
            Some(fqn) => fqn,
            None => return,
        };
        let diag_line = match data.extra.get("diagnostic_line").and_then(|v| v.as_u64()) {
            Some(l) => l as usize,
            None => return,
        };

        // Find the function body that contains the triggering diagnostic.
        let (func_start, func_end) = match find_enclosing_function_line_range(content, diag_line) {
            Some(range) => range,
            None => return,
        };

        let existing_diags = action.diagnostics.get_or_insert_with(Vec::new);

        let cache = self.phpstan_tool.last_diags.lock();
        let cached = match cache.get(&data.uri) {
            Some(c) => c,
            None => return,
        };

        for cached_d in cached {
            // Must be the same identifier.
            if phpstan_identifier(cached_d) != Some("missingType.checkedException") {
                continue;
            }

            // Must be for the same exception class.
            let cached_fqn: String =
                match extract_exception_fqn(crate::util::message_text(&cached_d.message)) {
                    Some(fqn) => fqn,
                    None => continue,
                };
            if !cached_fqn.eq_ignore_ascii_case(&exception_fqn) {
                continue;
            }

            let line = cached_d.range.start.line as usize;

            // Must be within the same function body.
            if line < func_start || line > func_end {
                continue;
            }

            // Skip if already in the list.
            let already_present = existing_diags.iter().any(|d| {
                d.range == cached_d.range
                    && d.message == cached_d.message
                    && d.code == cached_d.code
            });
            if already_present {
                continue;
            }

            existing_diags.push(cached_d.clone());
        }
    }

    /// Remove specific diagnostics from the PHPStan cache after a
    /// quickfix has been applied via `codeAction/resolve`.
    pub(crate) fn clear_phpstan_diagnostics_after_resolve(
        &self,
        uri: &str,
        resolved_diags: &[Diagnostic],
    ) {
        let mut cache = self.phpstan_tool.last_diags.lock();
        if let Some(cached) = cache.get_mut(uri) {
            cached.retain(|cached_d| {
                !resolved_diags.iter().any(|resolved_d| {
                    cached_d.range == resolved_d.range
                        && cached_d.message == resolved_d.message
                        && cached_d.code == resolved_d.code
                })
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_name_from_phpstan_messages() {
        for (message, expected) in [
            (
                "Method App\\Http\\UserController::index() overrides method App\\Http\\Controller::index() but is missing the #[\\Override] attribute.",
                Some("index"),
            ),
            (
                "Return type (int) of method Foo::count() should be covariant with return type (int) of method Countable::count()",
                Some("count"),
            ),
            (
                "Method App\\Foo::__construct() has #[\\Override] attribute but does not override any method.",
                Some("__construct"),
            ),
            (
                "Property App\\Foo::$baz has #[\\Override] attribute but does not override any property.",
                Some("$baz"),
            ),
            (
                "Attribute class Override can be used with properties only on PHP 8.5 and later.",
                None,
            ),
            ("Some other PHPStan error about something.", None),
        ] {
            assert_eq!(phpstan_member_name(message), expected, "{message}");
        }
    }

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
