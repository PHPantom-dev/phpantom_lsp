//! Staleness reconciliation for cached PHPStan diagnostics.
//!
//! PHPStan diagnostics are cached per-file and re-published on every
//! native diagnostic pass, but PHPStan itself only re-runs on its own
//! debounce timer. Between a user fixing an issue (via a code action or
//! a manual edit) and the next PHPStan run, the cached diagnostic would
//! otherwise linger. [`is_stale_phpstan_diagnostic`] detects the cases
//! where content-based evidence (an added `@phpstan-ignore` comment, a
//! fix applied by hand, …) shows the diagnostic no longer applies, so
//! `assemble_and_push` can prune it eagerly.

use tower_lsp::lsp_types::*;

/// Check whether a cached PHPStan diagnostic is stale given the current
/// file content.
///
/// A diagnostic is stale when the user has already fixed the underlying
/// issue (via a code action or manual edit) but PHPStan hasn't re-run
/// yet to clear it. Each branch below checks for the content-based
/// evidence a specific fix leaves behind (an attribute added, a PHPDoc
/// tag changed, a statement removed, …); see the comment above each
/// branch for details. Any identifier is also considered stale when the
/// diagnostic's line now carries a `@phpstan-ignore` comment listing it.
///
/// A few identifiers are deliberately not covered here: `throws.unusedType`
/// and `missingType.checkedException` are cleared eagerly by `codeAction/resolve` instead (see
/// `clear_phpstan_diagnostics_after_resolve` in `code_actions`);
/// `return.type` has no heuristic at all because no content-based check
/// can tell whether the right fix is to change the type or the code.
pub(crate) fn is_stale_phpstan_diagnostic(diag: &Diagnostic, content: &str) -> bool {
    let identifier = match &diag.code {
        Some(NumberOrString::String(s)) => s.as_str(),
        _ => return false,
    };

    // ── @phpstan-ignore covers this diagnostic ──────────────────────
    // If the line where the diagnostic appears now has a
    // `@phpstan-ignore` comment listing this identifier, the user
    // already suppressed it and the diagnostic is stale.
    if !identifier.is_empty()
        && identifier != "phpstan"
        && !identifier.starts_with("ignore.unmatched")
        && line_has_ignore_for(content, diag.range.start.line, identifier)
    {
        return true;
    }

    // ── method.override / property.override / property.overrideAttribute ─
    // The user may remove the attribute by hand, so check whether
    // `#[Override]` is still present near the diagnostic line.
    if identifier == "method.override"
        || identifier == "property.override"
        || identifier == "property.overrideAttribute"
    {
        return crate::code_actions::phpstan::remove_override::is_remove_override_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── method.missingOverride / method.tentativeReturnType ─────────
    // The user may add `#[\Override]` or `#[\ReturnTypeWillChange]` by
    // hand, so check whether it is now written on the method.
    if let Some(stale) = crate::code_actions::phpstan::add_attribute::is_add_attribute_stale(
        content,
        diag.range.start.line as usize,
        identifier,
    ) {
        return stale;
    }

    // ── PHPDoc type mismatch (return.phpDocType, parameter.phpDocType,
    //    property.phpDocType) — tag removed or type changed ──────────
    if identifier == "return.phpDocType"
        || identifier == "parameter.phpDocType"
        || identifier == "property.phpDocType"
    {
        return crate::code_actions::phpstan::fix_phpdoc_type::is_fix_phpdoc_type_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
            identifier,
        );
    }

    // ── new.static — check if the user manually fixed the class ─────
    // Unlike the actions above, `new.static` fixes are commonly applied
    // by hand (adding `final` to the class or constructor), so we keep
    // a content-based heuristic here.
    if identifier == "new.static" {
        return crate::code_actions::phpstan::new_static::is_new_static_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── class.prefixed — prefixed class name fixed ──────────────────
    // The user may fix the leading backslash by hand, so check whether
    // the prefixed name still appears on the diagnostic line.
    if identifier == "class.prefixed" {
        return crate::code_actions::phpstan::fix_prefixed_class::is_fix_prefixed_class_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
        );
    }

    // ── function.alreadyNarrowedType — always-true assert() removed ─
    // Only for `assert()` calls (not other functions sharing the same
    // identifier).  The diagnostic is stale when `assert(` no longer
    // appears on the diagnostic line.
    if identifier == "function.alreadyNarrowedType"
        && diag.message.starts_with("Call to function assert()")
    {
        return crate::code_actions::phpstan::remove_assert::is_remove_assert_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── return.void / return.empty / missingType.return ──────────────
    // Note: `return.type` is deliberately excluded — no content
    // heuristic can tell whether the right fix is to change the type
    // or change the code.  It is cleared eagerly by codeAction/resolve.
    if identifier == "return.void"
        || identifier == "return.empty"
        || identifier == "missingType.return"
    {
        return crate::code_actions::phpstan::fix_return_type::is_fix_return_type_stale(
            content,
            diag.range.start.line as usize,
            identifier,
            &diag.message,
        );
    }

    // ── deadCode.unreachable — unreachable statement removed ────────
    if identifier == "deadCode.unreachable" {
        return crate::code_actions::phpstan::remove_unreachable::is_remove_unreachable_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── missingType.iterableValue — @return with generic type added ─
    if identifier == "missingType.iterableValue" {
        return crate::code_actions::phpstan::add_iterable_type::is_add_iterable_type_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
        );
    }

    // ── return.unusedType — unused type removed from return type ─────
    if identifier == "return.unusedType" {
        return crate::code_actions::phpstan::remove_unused_return_type::is_remove_unused_return_type_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
        );
    }

    false
}

/// Check whether the diagnostic's line (or the line before it) has a
/// `@phpstan-ignore` comment that lists the given identifier.
///
/// PHPStan ignore comments can appear:
/// - On the same line as the code: `$x = foo(); // @phpstan-ignore id`
/// - On the line before: `// @phpstan-ignore id`
///
/// Only the per-identifier form (`@phpstan-ignore id1, id2`) is
/// checked.  The blanket `@phpstan-ignore-line` and
/// `@phpstan-ignore-next-line` variants are **not** treated as a
/// match — our code action only produces per-identifier ignores, so
/// we should not eagerly clear diagnostics that happen to sit on a
/// line with a blanket suppression the user added independently.
fn line_has_ignore_for(content: &str, diag_line: u32, identifier: &str) -> bool {
    let line_idx = diag_line as usize;

    // Check the diagnostic line itself and the line before it.
    // `@phpstan-ignore-line` and `@phpstan-ignore-next-line` list no
    // identifiers, so they never match.
    [line_idx, line_idx.wrapping_sub(1)].into_iter().any(|idx| {
        content.split('\n').nth(idx).is_some_and(|line| {
            crate::phpstan_ignore::parse_ignore_comment(line)
                .is_some_and(|comment| comment.ids(line).any(|id| id == identifier))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Range {
        Range {
            start: Position {
                line: start_line,
                character: start_char,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        }
    }

    // ── is_stale_phpstan_diagnostic ─────────────────────────────────

    /// Helper: build a PHPStan-style full-line diagnostic.
    fn make_phpstan_diag(line: u32, code: &str, message: &str) -> Diagnostic {
        Diagnostic {
            range: make_range(line, 0, line, 200),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(code.to_string())),
            source: Some("PHPStan".to_string()),
            message: message.to_string(),
            ..Default::default()
        }
    }

    // ── Per-identifier heuristics removed ───────────────────────────
    //
    // throws.unusedType, missingType.checkedException, and
    // method.missingOverride are cleared by `codeAction/resolve` instead
    // (see `clear_phpstan_diagnostics_after_resolve`), not by content
    // heuristics here.  The tests below verify that.

    #[test]
    fn throws_unused_type_not_stale_via_heuristic() {
        // Previously this was detected as stale because the @throws
        // tag was removed.  Now only codeAction/resolve clears it.
        let content = "<?php\nclass Foo {\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            2,
            "throws.unusedType",
            "Method App\\Foo::bar() has App\\Exceptions\\FooException in PHPDoc @throws tag but it's not thrown.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "throws.unusedType should NOT be stale via heuristic (cleared by resolve instead)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_mixed_reason_and_bare() {
        // First identifier has no reason, second has a reason.
        let content =
            "<?php\n$x = foo(); // @phpstan-ignore return.type, argument.type (not a real error)\n";
        let return_diag = make_phpstan_diag(1, "return.type", "...");
        let arg_diag = make_phpstan_diag(1, "argument.type", "...");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "bare identifier (no reason) should be stale"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "identifier with reason should be stale"
        );
    }

    #[test]
    fn stale_phpstan_ignore_reason_then_bare() {
        // First identifier has a reason, second is bare.
        let content =
            "<?php\n$x = foo(); // @phpstan-ignore return.type (some reason), argument.type\n";
        let return_diag = make_phpstan_diag(1, "return.type", "...");
        let arg_diag = make_phpstan_diag(1, "argument.type", "...");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "identifier with reason should be stale"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "bare identifier (no reason) after one with reason should be stale"
        );
    }

    #[test]
    fn missing_checked_exception_not_stale_via_heuristic() {
        // Previously this was detected as stale because a @throws
        // tag was added.  Now only codeAction/resolve clears it.
        let content = "<?php\nclass Foo {\n    /**\n     * @throws FooException\n     */\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            5,
            "missingType.checkedException",
            "Method App\\Foo::bar() throws checked exception App\\Exceptions\\FooException but it's missing from the PHPDoc @throws tag.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "missingType.checkedException should NOT be stale via heuristic (cleared by resolve instead)"
        );
    }

    #[test]
    fn stale_when_phpstan_ignore_covers_identifier() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore return.type\n}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale when @phpstan-ignore lists the identifier"
        );
    }

    #[test]
    fn not_stale_when_phpstan_ignore_covers_different_identifier() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore argument.type\n}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "should NOT be stale when @phpstan-ignore lists a different identifier"
        );
    }

    #[test]
    fn not_stale_for_phpstan_ignore_line_blanket() {
        // @phpstan-ignore-line suppresses everything, but we don't
        // eagerly prune for it — only per-identifier ignores count.
        let content =
            "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore-line\n}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "should NOT be stale for blanket @phpstan-ignore-line"
        );
    }

    // ── method.missingOverride stale detection ──────────────────────

    // ── method.missingOverride — heuristic removed ──────────────────

    #[test]
    fn missing_override_stale_once_the_attribute_is_written() {
        let content = "<?php\nclass Foo extends Bar {\n    #[\\Override]\n    public function baz(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "method.missingOverride",
            "Method Foo::baz() overrides method Bar::baz() but is missing the #[\\Override] attribute.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "method.missingOverride should be stale once #[\\Override] is written"
        );
    }

    #[test]
    fn not_stale_for_phpstan_ignore_next_line_blanket() {
        let content = "<?php\nclass Foo {\n    // @phpstan-ignore-next-line\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "should NOT be stale for blanket @phpstan-ignore-next-line"
        );
    }

    #[test]
    fn stale_when_phpstan_ignore_on_previous_line() {
        let content = "<?php\nclass Foo {\n    // @phpstan-ignore return.type\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale when @phpstan-ignore on previous line lists the identifier"
        );
    }

    #[test]
    fn stale_phpstan_ignore_with_multiple_ids() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore return.type, argument.type\n}\n";
        let return_diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        let arg_diag = make_phpstan_diag(
            2,
            "argument.type",
            "Parameter #1 $x expects string, int given.",
        );
        let other_diag = make_phpstan_diag(2, "method.notFound", "Call to undefined method.");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "return.type should be stale (listed in ignore)"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "argument.type should be stale (listed in ignore)"
        );
        assert!(
            !is_stale_phpstan_diagnostic(&other_diag, content),
            "method.notFound should NOT be stale (not listed)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_with_reason_in_docblock() {
        // `/** @phpstan-ignore id (reason) */` — the `*/` comes after
        // the reason text, so the parser must not treat everything
        // up to `*/` as the identifier list.
        let content = "<?php\nclass Foo {\n    /** @phpstan-ignore return.type (not a real error) */\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale when @phpstan-ignore in docblock has a (reason)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_with_per_id_reasons() {
        // Each identifier has its own `(reason)`:
        //   @phpstan-ignore return.type (reason1), argument.type (reason2)
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore return.type (not real), argument.type (also fine)\n}\n";
        let return_diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        let arg_diag = make_phpstan_diag(
            2,
            "argument.type",
            "Parameter #1 $x expects string, int given.",
        );
        let other_diag = make_phpstan_diag(2, "method.notFound", "Call to undefined method.");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "return.type should be stale (first in list with reason)"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "argument.type should be stale (second in list with reason)"
        );
        assert!(
            !is_stale_phpstan_diagnostic(&other_diag, content),
            "method.notFound should NOT be stale (not listed)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_reason_with_commas() {
        // The reason text contains commas — must not confuse the parser.
        let content = "<?php\n/** @phpstan-ignore return.type (accepts A, B, and C but PHPDoc is narrower) */\nfunction foo(): void {}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Function foo() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale even when reason text contains commas"
        );
    }

    #[test]
    fn stale_phpstan_ignore_multiple_ids_each_with_comma_reasons() {
        // Multiple identifiers, each with reason text that contains commas.
        // This is the trickiest case: the parser must not confuse commas
        // inside `(reason)` with the comma separating identifiers.
        let content = "<?php\n// @phpstan-ignore return.type (accepts A, B, C), argument.type (needs X, Y)\nfunction foo(): void {}\n";
        let return_diag = make_phpstan_diag(
            1,
            "return.type",
            "Function foo() should return string but returns void.",
        );
        let arg_diag = make_phpstan_diag(
            1,
            "argument.type",
            "Parameter #1 $x expects string, int given.",
        );
        let other_diag = make_phpstan_diag(1, "method.notFound", "Call to undefined method.");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "return.type should be stale (first id, reason has commas)"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "argument.type should be stale (second id, reason has commas)"
        );
        assert!(
            !is_stale_phpstan_diagnostic(&other_diag, content),
            "method.notFound should NOT be stale (not listed)"
        );
    }

    #[test]
    fn diag_with_no_code_is_never_stale() {
        let content = "<?php\n// @phpstan-ignore return.type\nfoo();";
        let diag = Diagnostic {
            range: make_range(1, 0, 1, 200),
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            source: Some("PHPStan".to_string()),
            message: "Some error.".to_string(),
            ..Default::default()
        };
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "diagnostic without a code should never be considered stale"
        );
    }

    #[test]
    fn ignore_unmatched_diag_is_never_stale_via_ignore_check() {
        // ignore.unmatched diagnostics should not be pruned by the
        // @phpstan-ignore check (they ARE the ignore comment).
        let content = "<?php\n$x = 1; // @phpstan-ignore ignore.unmatchedIdentifier\n";
        let diag = make_phpstan_diag(
            1,
            "ignore.unmatchedIdentifier",
            "No error with identifier foo is reported on line 2.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "ignore.unmatched* diagnostics must not be pruned by the ignore check"
        );
    }

    // ── Scoped docblock checks ──────────────────────────────────────
    //
    // These tests verify that throws/override diagnostics are never
    // stale via content heuristic (they are cleared by
    // codeAction/resolve instead).

    #[test]
    fn throws_not_stale_even_when_tag_on_same_function() {
        // Previously this was stale because @throws FooException was
        // found on baz()'s own docblock.  Now it's not — resolve
        // handles clearing.
        let content = concat!(
            "<?php\nclass Foo {\n",
            "    public function bar(): void {}\n",
            "    /**\n",
            "     * @throws FooException\n",
            "     */\n",
            "    public function baz(): void {\n",
            "        throw new FooException();\n",
            "    }\n",
            "}\n",
        );
        let diag = make_phpstan_diag(
            7,
            "missingType.checkedException",
            "Method App\\Foo::baz() throws checked exception App\\Exceptions\\FooException but it's missing from the PHPDoc @throws tag.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "missingType.checkedException should NOT be stale via heuristic"
        );
    }

    #[test]
    fn unused_throws_not_stale_via_heuristic_even_when_tag_removed() {
        // Previously baz()'s diagnostic was stale because the tag was
        // removed.  Now neither is stale via heuristic.
        let content = concat!(
            "<?php\nclass Foo {\n",
            "    /**\n",
            "     * @throws FooException\n",
            "     */\n",
            "    public function bar(): void {\n",
            "    }\n",
            "    public function baz(): void {\n",
            "    }\n",
            "}\n",
        );
        let bar_diag = make_phpstan_diag(
            5,
            "throws.unusedType",
            "Method App\\Foo::bar() has App\\Exceptions\\FooException in PHPDoc @throws tag but it's not thrown.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&bar_diag, content),
            "bar()'s throws.unusedType should NOT be stale via heuristic"
        );

        let baz_diag = make_phpstan_diag(
            7,
            "throws.unusedType",
            "Method App\\Foo::baz() has App\\Exceptions\\FooException in PHPDoc @throws tag but it's not thrown.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&baz_diag, content),
            "baz()'s throws.unusedType should NOT be stale via heuristic"
        );
    }
}
