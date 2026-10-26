//! Post-processing suppression policy applied to the merged diagnostic
//! set before it is delivered to the editor: eager code-action
//! suppression, cross-source overlap deduplication/ordering, and
//! `@phpantom-ignore` comment suppression.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::text_position::ranges_overlap;

impl Backend {
    /// Remove diagnostics that were eagerly suppressed by a
    /// `codeAction/resolve` handler and drain the suppression list.
    ///
    /// This is called during `assemble_and_push` so that the squiggly
    /// line disappears before the text edit is applied.
    pub(crate) fn filter_suppressed(&self, mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let mut suppressed = self.diag.suppressed.lock();
        if suppressed.is_empty() {
            return diagnostics;
        }
        diagnostics.retain(|d| {
            !suppressed
                .iter()
                .any(|s| d.range == s.range && d.message == s.message && d.code == s.code)
        });
        suppressed.clear();
        diagnostics
    }
}

/// Drop `unresolved_member_access` hints that are already explained by
/// a more precise diagnostic, then order the survivors for display.
///
/// **Suppression.**  When a priority diagnostic (`unknown_class`,
/// `unknown_member`, `scalar_member_access`, `unknown_function`)
/// overlaps an `unresolved_member_access` hint, the hint is dropped
/// because the root cause is already surfaced by the priority
/// diagnostic.  This is the only case we remove: it is a strict
/// refinement of the same finding, not two independent issues.
///
/// We do **not** discard overlapping external (line-only) diagnostics
/// this way.  Diagnostics arrive from multiple independent sources
/// (Mago parser, PHPStan, native PHPantom checks) that use completely
/// different error codes and descriptions, so there is no reliable way
/// to tell whether two diagnostics from different sources describe the
/// same issue — a full-line PHPStan/PHPCS/Mago finding can be an
/// independent issue (and may be more severe) than a precise native
/// one on the same line, so hiding it risks losing a critical error
/// behind a minor note.
///
/// **Ordering.**  Tools like PHPStan only report a line number, so
/// their diagnostics span the entire line (character 0 to a very large
/// end character), while native diagnostics and parser errors pinpoint
/// the exact token.  Rather than hide the full-line ones, the sort at
/// the end keeps a precise marker from being buried under a full-line
/// underline (see the sort's inline comment).
///
/// This is **not** deduplication in the traditional sense (removing
/// identical entries).  Each diagnostic source fully replaces its own
/// cache on every run, so true duplicates across sources do not occur.
pub(crate) fn suppress_imprecise_overlaps(diagnostics: &mut Vec<Diagnostic>) {
    if diagnostics.is_empty() {
        return;
    }

    // Collect the ranges of "priority" diagnostics that should
    // suppress `unresolved_member_access` hints.
    let priority_codes: &[&str] = &[
        "unknown_class",
        "unknown_member",
        "scalar_member_access",
        "unknown_function",
    ];

    let priority_ranges: Vec<Range> = diagnostics
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .map(|c| match c {
                    Code::String(s) => priority_codes.contains(&s.as_str()),
                    _ => false,
                })
                .unwrap_or(false)
        })
        .map(|d| d.range)
        .collect();

    diagnostics.retain(|d| {
        let is_unresolved = d
            .code
            .as_ref()
            .map(|c| match c {
                Code::String(s) => s == "unresolved_member_access",
                _ => false,
            })
            .unwrap_or(false);

        if is_unresolved {
            // Suppress if any priority diagnostic overlaps this range.
            return !priority_ranges
                .iter()
                .any(|pr| ranges_overlap(pr, &d.range));
        }

        true
    });

    // Order diagnostics so that, at a shared location, the most useful
    // one is listed first.  We no longer hide overlapping external
    // (line-only) diagnostics, so ordering is what keeps a precise
    // marker from being buried under a full-line underline in the
    // editor.  Within a line: most-severe first (a critical error leads
    // regardless of which tool found it), then precise before full-line
    // (a pinpointed range beats a whole-line span), then left-to-right
    // by start, then by end for a stable order.
    diagnostics.sort_by(|a, b| {
        a.range
            .start
            .line
            .cmp(&b.range.start.line)
            .then_with(|| severity_rank(a).cmp(&severity_rank(b)))
            .then_with(|| is_full_line_range(&a.range).cmp(&is_full_line_range(&b.range)))
            .then_with(|| a.range.start.character.cmp(&b.range.start.character))
            .then_with(|| a.range.end.line.cmp(&b.range.end.line))
            .then_with(|| a.range.end.character.cmp(&b.range.end.character))
    });
}

/// Ranks a diagnostic's severity for ordering, most severe first
/// (`ERROR` → 0, `WARNING` → 1, `INFORMATION` → 2, `HINT` → 3).  A
/// missing severity sorts last so explicitly-classified diagnostics
/// take precedence.
fn severity_rank(d: &Diagnostic) -> u8 {
    match d.severity {
        Some(DiagnosticSeverity::Error) => 0,
        Some(DiagnosticSeverity::Warning) => 1,
        Some(DiagnosticSeverity::Information) => 2,
        Some(DiagnosticSeverity::Hint) => 3,
        _ => 4,
    }
}

/// Returns `true` if the range spans a full line (character 0 to a
/// very large end character).  PHPStan and other line-only tools
/// produce these ranges because they don't report column information.
/// Used only for ordering (full-line diagnostics sort after precise
/// ones on the same line), never to suppress them.
fn is_full_line_range(range: &Range) -> bool {
    range.start.line == range.end.line && range.start.character == 0 && range.end.character >= 1000
}

/// Remove diagnostics suppressed by `@phpantom-ignore` comments.
///
/// Supports two forms:
/// - **Same-line:** `$x->foo; // @phpantom-ignore unknown_member`
/// - **Next-line:** `// @phpantom-ignore unused_variable` on the line above.
///
/// Multiple codes can be comma-separated:
/// `// @phpantom-ignore unknown_member, unused_variable`
///
/// A bare `@phpantom-ignore` (no codes) suppresses ALL diagnostics on
/// that line.
pub(crate) fn filter_ignored_by_comment(diagnostics: &mut Vec<Diagnostic>, content: &str) {
    if diagnostics.is_empty() {
        return;
    }

    // Pre-compute per-line ignore sets.  A `None` value means "ignore all".
    // A `Some(set)` means only ignore those specific codes.
    let lines: Vec<&str> = content.lines().collect();
    let mut ignore_map: std::collections::HashMap<u32, Option<Vec<&str>>> =
        std::collections::HashMap::new();

    for (line_idx, line_text) in lines.iter().enumerate() {
        if let Some(ignore_pos) = line_text.find("@phpantom-ignore") {
            let after = &line_text[ignore_pos + "@phpantom-ignore".len()..];

            // Check this isn't `@phpantom-ignore-` (future extensions).
            if after.starts_with('-') {
                continue;
            }

            let codes: Option<Vec<&str>> = {
                let trimmed = after.trim();
                if trimmed.is_empty() || trimmed.starts_with("*/") {
                    None // bare ignore = suppress all
                } else {
                    // Strip trailing */ for block comments
                    let trimmed = trimmed.trim_end_matches("*/").trim();
                    Some(
                        trimmed
                            .split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    )
                }
            };

            // Determine whether this is a same-line or next-line ignore.
            // If the comment is the only non-whitespace on the line
            // (after stripping the `//` or `/*` prefix), it applies to
            // the next line.  Otherwise it applies to the same line.
            let before_comment = &line_text[..ignore_pos];
            let is_standalone = before_comment
                .trim()
                .trim_start_matches("//")
                .trim_start_matches("/*")
                .trim_start_matches('*')
                .trim()
                .is_empty();

            let target_line = if is_standalone {
                line_idx as u32 + 1 // next line
            } else {
                line_idx as u32 // same line
            };

            ignore_map.insert(target_line, codes);
        }
    }

    if ignore_map.is_empty() {
        return;
    }

    diagnostics.retain(|d| {
        let line = d.range.start.line;
        if let Some(codes) = ignore_map.get(&line) {
            match codes {
                None => false, // suppress all
                Some(code_list) => {
                    // Check if this diagnostic's code is in the list.
                    let diag_code = d.code.as_ref().and_then(|c| match c {
                        Code::String(s) => Some(s.as_str()),
                        _ => None,
                    });
                    if let Some(dc) = diag_code {
                        !code_list.contains(&dc)
                    } else {
                        true // no code = can't suppress
                    }
                }
            }
        } else {
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ─────────────────────────────────────────────────────

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

    fn make_diagnostic(
        range: Range,
        severity: DiagnosticSeverity,
        code: &str,
        message: &str,
    ) -> Diagnostic {
        Diagnostic {
            range,
            severity: Some(severity),
            code: Some(Code::String(code.to_string())),
            code_description: None,
            source: Some("phpantom".to_string()),
            message: message.to_string().into(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    // ── ranges_overlap ──────────────────────────────────────────────

    #[test]
    fn overlapping_ranges_on_same_line() {
        let a = make_range(5, 0, 5, 10);
        let b = make_range(5, 5, 5, 15);
        assert!(ranges_overlap(&a, &b));
        assert!(ranges_overlap(&b, &a));
    }

    #[test]
    fn non_overlapping_ranges_on_same_line() {
        let a = make_range(5, 0, 5, 5);
        let b = make_range(5, 5, 5, 10);
        assert!(!ranges_overlap(&a, &b));
        assert!(!ranges_overlap(&b, &a));
    }

    #[test]
    fn non_overlapping_ranges_on_different_lines() {
        let a = make_range(1, 0, 1, 10);
        let b = make_range(2, 0, 2, 10);
        assert!(!ranges_overlap(&a, &b));
    }

    #[test]
    fn identical_ranges_overlap() {
        let r = make_range(3, 5, 3, 10);
        assert!(ranges_overlap(&r, &r));
    }

    #[test]
    fn contained_range_overlaps() {
        let outer = make_range(1, 0, 10, 0);
        let inner = make_range(5, 5, 5, 10);
        assert!(ranges_overlap(&outer, &inner));
        assert!(ranges_overlap(&inner, &outer));
    }

    // ── suppress_imprecise_overlaps ─────────────────────────────────

    #[test]
    fn suppresses_unresolved_member_when_unknown_class_overlaps() {
        let range = make_range(5, 0, 5, 15);
        let mut diags = vec![
            make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                "unknown_class",
                "Unknown class X",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::Hint,
                "unresolved_member_access",
                "Unresolved member access on X",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(Code::String("unknown_class".to_string()))
        );
    }

    #[test]
    fn suppresses_unresolved_member_when_unknown_member_overlaps() {
        let range = make_range(10, 0, 10, 20);
        let mut diags = vec![
            make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                "unknown_member",
                "Unknown member foo",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::Hint,
                "unresolved_member_access",
                "Unresolved member access",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(Code::String("unknown_member".to_string()))
        );
    }

    #[test]
    fn suppresses_unresolved_member_when_scalar_member_access_overlaps() {
        let range_outer = make_range(3, 0, 3, 20);
        let range_inner = make_range(3, 5, 3, 15);
        let mut diags = vec![
            make_diagnostic(
                range_outer,
                DiagnosticSeverity::Error,
                "scalar_member_access",
                "Cannot access member on scalar",
            ),
            make_diagnostic(
                range_inner,
                DiagnosticSeverity::Hint,
                "unresolved_member_access",
                "Unresolved member access",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(Code::String("scalar_member_access".to_string()))
        );
    }

    #[test]
    fn keeps_unresolved_member_when_no_priority_diagnostic() {
        let range = make_range(5, 0, 5, 15);
        let mut diags = vec![make_diagnostic(
            range,
            DiagnosticSeverity::Hint,
            "unresolved_member_access",
            "Unresolved member access",
        )];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn keeps_unresolved_member_on_different_range() {
        let mut diags = vec![
            make_diagnostic(
                make_range(5, 0, 5, 10),
                DiagnosticSeverity::Warning,
                "unknown_class",
                "Unknown class X",
            ),
            make_diagnostic(
                make_range(10, 0, 10, 10),
                DiagnosticSeverity::Hint,
                "unresolved_member_access",
                "Unresolved member access on Y",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn suppresses_multiple_unresolved_members_with_priority_overlap() {
        let range = make_range(5, 0, 5, 15);
        let mut diags = vec![
            make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                "unknown_class",
                "Unknown class X",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::Hint,
                "unresolved_member_access",
                "Unresolved 1",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::Hint,
                "unresolved_member_access",
                "Unresolved 2",
            ),
            make_diagnostic(
                make_range(20, 0, 20, 10),
                DiagnosticSeverity::Hint,
                "unresolved_member_access",
                "Unresolved 3 (different range)",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        // Only the unknown_class + the one on a different range should survive.
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn no_op_when_no_diagnostics() {
        let mut diags: Vec<Diagnostic> = vec![];
        suppress_imprecise_overlaps(&mut diags);
        assert!(diags.is_empty());
    }

    #[test]
    fn keeps_full_line_phpstan_with_precise_diagnostic_on_same_line() {
        let phpstan = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Parameter #1 $x expects int, string given."
                .to_string()
                .into(),
            ..Default::default()
        };
        let precise = Diagnostic {
            range: make_range(5, 10, 5, 20),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("unknown_class".to_string())),
            source: Some("phpantom".to_string()),
            message: "Class 'Foo' not found".to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![phpstan.clone(), precise.clone()];
        suppress_imprecise_overlaps(&mut diags);
        // Both survive, and the precise marker is listed first so it is
        // not buried under the full-line underline in the editor.
        assert_eq!(diags, vec![precise, phpstan]);
    }

    #[test]
    fn most_severe_diagnostic_leads_on_a_shared_line() {
        // A full-line error must sort ahead of a precise warning on the
        // same line: the critical finding is surfaced first regardless
        // of which tool reported it or how precise its range is.
        let full_line_error = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Parameter #1 $x expects int, string given."
                .to_string()
                .into(),
            ..Default::default()
        };
        let precise_warning = Diagnostic {
            range: make_range(5, 10, 5, 20),
            severity: Some(DiagnosticSeverity::Warning),
            code: Some(Code::String("unknown_member".to_string())),
            source: Some("phpantom".to_string()),
            message: "Method 'foo' not found".to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![precise_warning.clone(), full_line_error.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags, vec![full_line_error, precise_warning]);
    }

    #[test]
    fn keeps_phpstan_type_dump_with_unknown_function() {
        let dump = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("phpstan.dumpType".to_string())),
            source: Some("phpstan".to_string()),
            message: "Dumped type: Foo".to_string().into(),
            ..Default::default()
        };
        let unknown_function = Diagnostic {
            range: make_range(5, 0, 5, 19),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("unknown_function".to_string())),
            source: Some("phpantom".to_string()),
            message: "Unknown function PHPStan\\dumpType".to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![dump.clone(), unknown_function.clone()];

        suppress_imprecise_overlaps(&mut diags);

        assert_eq!(diags, vec![unknown_function, dump]);
    }

    #[test]
    fn keeps_full_line_diagnostic_regardless_of_code() {
        let phpstan = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("class.prefixed".to_string())),
            source: Some("phpstan".to_string()),
            message: "Class prefixed with vendor namespace.".to_string().into(),
            ..Default::default()
        };
        let syntax_error = Diagnostic {
            range: make_range(5, 3, 5, 10),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("syntax_error".to_string())),
            source: Some("phpantom".to_string()),
            message: "Syntax error: unexpected token `->`".to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![phpstan, syntax_error.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn keeps_full_line_phpstan_when_no_precise_diagnostic_on_line() {
        let phpstan = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Parameter #1 $x expects int, string given."
                .to_string()
                .into(),
            ..Default::default()
        };
        let precise_other_line = Diagnostic {
            range: make_range(10, 3, 10, 15),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("unknown_class".to_string())),
            source: Some("phpantom".to_string()),
            message: "Class 'Bar' not found".to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![phpstan.clone(), precise_other_line.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn keeps_precise_phpstan_diagnostic_on_same_line() {
        // If a future PHPStan version provides column info, don't suppress it.
        let phpstan_precise = Diagnostic {
            range: make_range(5, 8, 5, 20),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Parameter #1 $x expects int, string given."
                .to_string()
                .into(),
            ..Default::default()
        };
        let native_precise = Diagnostic {
            range: make_range(5, 3, 5, 10),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("unknown_class".to_string())),
            source: Some("phpantom".to_string()),
            message: "Class 'Foo' not found".to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![phpstan_precise.clone(), native_precise.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn keeps_multiple_full_line_diags_when_precise_exists() {
        let phpstan1 = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Error one".to_string().into(),
            ..Default::default()
        };
        let phpstan2 = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("return.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Error two".to_string().into(),
            ..Default::default()
        };
        let precise = Diagnostic {
            range: make_range(5, 2, 5, 8),
            severity: Some(DiagnosticSeverity::Warning),
            code: Some(Code::String("unknown_member".to_string())),
            source: Some("phpantom".to_string()),
            message: "Method 'foo' not found".to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![phpstan1, phpstan2, precise.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn keeps_multiple_diagnostics_on_same_range() {
        // Each source is authoritative — two PHPantom diagnostics on
        // the same span are both shown.
        let range = make_range(7, 3, 7, 12);
        let diag1 = make_diagnostic(
            range,
            DiagnosticSeverity::Warning,
            "unknown_member",
            "Method 'foo' not found on class Bar",
        );
        let diag2 = make_diagnostic(
            range,
            DiagnosticSeverity::Hint,
            "deprecated_usage",
            "Method 'foo' is deprecated",
        );
        let mut diags = vec![diag1, diag2];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn keeps_multiple_phpstan_diagnostics_on_same_line() {
        // If PHPStan reports five issues on a line and no precise
        // diagnostic exists, all five survive.
        let make_phpstan = |code: &str, msg: &str| Diagnostic {
            range: make_range(10, 0, 10, u32::MAX),
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String(code.to_string())),
            source: Some("phpstan".to_string()),
            message: msg.to_string().into(),
            ..Default::default()
        };
        let mut diags = vec![
            make_phpstan("argument.type", "Parameter #1 expects int, string given."),
            make_phpstan("return.type", "Should return int but returns string."),
            make_phpstan("missingType.return", "Method has no return type."),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 3);
    }
}
