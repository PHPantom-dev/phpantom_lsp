//! "Fix prefixed class name" code action for PHPStan `class.prefixed`.
//!
//! When PHPStan reports that code references a class with an internal
//! vendor prefix (e.g. `_PHPStan_foo\SomeClass` instead of `\SomeClass`),
//! this code action offers to replace the prefixed name with the
//! corrected one.
//!
//! The prefixed name is extracted from the diagnostic message, which has
//! the format: `Referencing prefixed {project} class: {prefixedName}.`
//!
//! The corrected name is extracted from the diagnostic tip, which has
//! the format:
//! `This is most likely unintentional. Did you mean to type {corrected}?`
//!
//! Known vendor prefixes (from PHPStan source):
//! - `_PHPStan_` (PHPStan)
//! - `RectorPrefix` (Rector)
//! - `_PhpScoper` (PHP-Scoper)
//! - `PHPUnitPHAR` (PHPUnit)
//! - `_HumbugBox` (Box/Humbug)
//!
//! **Trigger:** A PHPStan diagnostic with identifier `class.prefixed`
//! overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! **Non-ignorable:** This diagnostic cannot be suppressed with
//! `@phpstan-ignore`, so the code action is the primary way to fix it.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_fix_prefixed_class_actions`) validates that the
//! action is applicable and emits a lightweight `CodeAction` with a
//! `data` payload but no `edit`.  Phase 2 (`resolve_fix_prefixed_class`)
//! recomputes the workspace edit on demand when the user picks the
//! action.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::text_position::ranges_overlap;

use super::split_phpstan_tip;

// ── PHPStan identifier ──────────────────────────────────────────────────────

/// PHPStan identifier for the "prefixed class name" diagnostic.
const CLASS_PREFIXED_ID: &str = "class.prefixed";

/// Action kind string for the resolve dispatch table.
const ACTION_KIND: &str = "phpstan.fixPrefixedClass";

// ── Parsed diagnostic ───────────────────────────────────────────────────────

/// Information extracted from a `class.prefixed` diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PrefixedClassInfo {
    /// The full vendor-prefixed class name (e.g.
    /// `_PHPStan_foo\SomeClass`).
    prefixed: String,
    /// The corrected class name from the tip (e.g. `\SomeClass`).
    corrected: String,
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Fix prefixed class name" code actions for PHPStan
    /// `class.prefixed` diagnostics.
    pub(crate) fn collect_fix_prefixed_class_actions(
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

            if identifier != CLASS_PREFIXED_ID {
                continue;
            }

            let info = match parse_prefixed_diagnostic(&diag.message) {
                Some(i) => i,
                None => continue,
            };

            let diag_line = diag.range.start.line as usize;

            // Verify that the prefixed name actually appears on the
            // diagnostic line.  PHP code may use the fully-qualified
            // form with a leading backslash (`\_PHPStan_test\Foo`)
            // even though PHPStan's message omits it
            // (`_PHPStan_test\Foo`).  Try both forms.
            let lines: Vec<&str> = content.lines().collect();
            if diag_line >= lines.len() {
                continue;
            }
            let line_text = lines[diag_line];
            let fqn_prefixed = format!("\\{}", info.prefixed);
            let actual_prefixed = if find_occurrence(line_text, &fqn_prefixed).is_some() {
                fqn_prefixed
            } else if find_occurrence(line_text, &info.prefixed).is_some() {
                info.prefixed.clone()
            } else {
                continue;
            };

            let title = format!("Replace {} with {}", actual_prefixed, info.corrected);

            let extra = serde_json::json!({
                "diagnostic_message": diag.message,
                "diagnostic_line": diag_line,
                "diagnostic_code": CLASS_PREFIXED_ID,
                "prefixed_name": actual_prefixed,
                "corrected_name": info.corrected,
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

    /// Resolve a "Fix prefixed class name" code action by computing the
    /// full workspace edit.
    pub(crate) fn resolve_fix_prefixed_class(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let extra = &data.extra;
        let diag_line = extra.get("diagnostic_line")?.as_u64()? as usize;
        let prefixed = extra.get("prefixed_name")?.as_str()?;
        let corrected = extra.get("corrected_name")?.as_str()?;

        let edit = build_fix_prefixed_edit(content, diag_line, prefixed, corrected)?;

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, vec![edit]);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

// ── Message + tip parsing ───────────────────────────────────────────────────

/// Parse a `class.prefixed` diagnostic to extract the prefixed and
/// corrected class names.
///
/// **Message format:**
/// `Referencing prefixed {project} class: {prefixedName}.`
///
/// **Tip format:**
/// `This is most likely unintentional. Did you mean to type {corrected}?`
fn parse_prefixed_diagnostic(message: &str) -> Option<PrefixedClassInfo> {
    let prefixed = extract_prefixed_name(message)?;
    let corrected = extract_corrected_name(message)?;

    Some(PrefixedClassInfo {
        prefixed,
        corrected,
    })
}

/// Extract the vendor-prefixed class name from the diagnostic message.
///
/// Message format: `Referencing prefixed {project} class: {prefixedName}.`
fn extract_prefixed_name(message: &str) -> Option<String> {
    let (msg, _tip) = split_phpstan_tip(message);

    let marker = " class: ";
    let start = msg.find(marker)? + marker.len();
    let rest = &msg[start..];

    // The prefixed name ends at the trailing period.
    let name = rest.trim_end_matches('.');
    if name.is_empty() {
        return None;
    }

    Some(name.to_string())
}

/// Extract the corrected class name from the diagnostic tip.
///
/// Tip format:
/// `This is most likely unintentional. Did you mean to type {corrected}?`
fn extract_corrected_name(message: &str) -> Option<String> {
    let (_msg, tip) = split_phpstan_tip(message);
    let tip = tip?;

    let marker = "Did you mean to type ";
    let start = tip.find(marker)? + marker.len();
    let rest = &tip[start..];

    let end = rest.rfind('?')?;
    let name = rest[..end].trim();

    if name.is_empty() {
        return None;
    }

    Some(name.to_string())
}

// ── Edit builder ────────────────────────────────────────────────────────────

/// Build a `TextEdit` that replaces the first occurrence of the
/// vendor-prefixed class name with the corrected name on the diagnostic
/// line.
fn build_fix_prefixed_edit(
    content: &str,
    diag_line: usize,
    prefixed: &str,
    corrected: &str,
) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let line_text = lines[diag_line];

    // Find the byte offset of the prefixed name within the line.
    let byte_col = find_occurrence(line_text, prefixed)?;

    // Convert byte offsets to UTF-16 code unit offsets for LSP.
    let start_char = byte_offset_to_utf16(line_text, byte_col);
    let end_char = byte_offset_to_utf16(line_text, byte_col + prefixed.len());

    Some(TextEdit {
        range: Range {
            start: Position {
                line: diag_line as u32,
                character: start_char,
            },
            end: Position {
                line: diag_line as u32,
                character: end_char,
            },
        },
        new_text: corrected.to_string(),
    })
}

/// Find the byte offset of a class name within a line.
///
/// We look for the exact `name` string and verify that it is not part
/// of a longer identifier (the character before should not be an
/// identifier char, and the character after should not be an identifier
/// continuation).
fn find_occurrence(line: &str, name: &str) -> Option<usize> {
    let mut search_start = 0;
    while let Some(pos) = line[search_start..].find(name) {
        let abs_pos = search_start + pos;

        // Check that this is not in the middle of a longer identifier.
        if abs_pos > 0 {
            let prev_byte = line.as_bytes()[abs_pos - 1];
            if prev_byte.is_ascii_alphanumeric() || prev_byte == b'_' || prev_byte == b'\\' {
                search_start = abs_pos + 1;
                continue;
            }
        }

        // Check that the character after the match is not an identifier
        // continuation.
        let end_pos = abs_pos + name.len();
        if end_pos < line.len() {
            let next_byte = line.as_bytes()[end_pos];
            if next_byte.is_ascii_alphanumeric() || next_byte == b'_' {
                search_start = abs_pos + 1;
                continue;
            }
        }

        return Some(abs_pos);
    }

    None
}

/// Convert a byte offset within a line to a UTF-16 code unit offset.
fn byte_offset_to_utf16(line: &str, byte_offset: usize) -> u32 {
    let prefix = &line[..byte_offset.min(line.len())];
    prefix.encode_utf16().count() as u32
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a `class.prefixed` diagnostic is stale.
///
/// A diagnostic is stale when the vendor-prefixed class name no longer
/// appears on the diagnostic line (in either bare or FQN form).
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/mod.rs`.
pub(crate) fn is_fix_prefixed_class_stale(content: &str, diag_line: usize, message: &str) -> bool {
    let prefixed = match extract_prefixed_name(message) {
        Some(name) => name,
        None => return false,
    };

    let lines: Vec<&str> = content.lines().collect();

    if diag_line >= lines.len() {
        // Line no longer exists — definitely stale.
        return true;
    }

    let line_text = lines[diag_line];
    let fqn_prefixed = format!("\\{}", prefixed);

    // The diagnostic is stale if neither the bare nor the FQN form
    // of the prefixed name appears on the diagnostic line.
    find_occurrence(line_text, &prefixed).is_none()
        && find_occurrence(line_text, &fqn_prefixed).is_none()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Diagnostic message used across tests ────────────────────────

    /// A realistic `class.prefixed` message + tip for PHPStan-scoped class.
    const PHPSTAN_MSG: &str = "Referencing prefixed PHPStan class: _PHPStan_test\\SomeClass.\n\
                               This is most likely unintentional. Did you mean to type \\SomeClass?";

    /// Rector-scoped class.
    const RECTOR_MSG: &str = "Referencing prefixed Rector class: RectorPrefix202501\\Symfony\\Component\\Console\\Application.\n\
                              This is most likely unintentional. Did you mean to type \\Symfony\\Component\\Console\\Application?";

    /// PHP-Scoper prefixed class.
    const SCOPER_MSG: &str = "Referencing prefixed PHP-Scoper class: _PhpScoper123\\GuzzleHttp\\Client.\n\
                              This is most likely unintentional. Did you mean to type \\GuzzleHttp\\Client?";

    /// PHPUnit PHAR prefixed class.
    const PHPUNIT_MSG: &str = "Referencing prefixed PHPUnit class: PHPUnitPHAR\\SebastianBergmann\\Diff\\Differ.\n\
                               This is most likely unintentional. Did you mean to type \\SebastianBergmann\\Diff\\Differ?";

    /// Humbug Box prefixed class.
    const HUMBUG_MSG: &str = "Referencing prefixed Box class: _HumbugBox456\\Composer\\Autoload\\ClassLoader.\n\
                              This is most likely unintentional. Did you mean to type \\Composer\\Autoload\\ClassLoader?";

    // ── extract_prefixed_name ───────────────────────────────────────

    #[test]
    fn extracts_phpstan_prefixed_name() {
        assert_eq!(
            extract_prefixed_name(PHPSTAN_MSG),
            Some("_PHPStan_test\\SomeClass".into())
        );
    }

    #[test]
    fn extracts_rector_prefixed_name() {
        assert_eq!(
            extract_prefixed_name(RECTOR_MSG),
            Some("RectorPrefix202501\\Symfony\\Component\\Console\\Application".into())
        );
    }

    #[test]
    fn extracts_scoper_prefixed_name() {
        assert_eq!(
            extract_prefixed_name(SCOPER_MSG),
            Some("_PhpScoper123\\GuzzleHttp\\Client".into())
        );
    }

    #[test]
    fn extracts_phpunit_prefixed_name() {
        assert_eq!(
            extract_prefixed_name(PHPUNIT_MSG),
            Some("PHPUnitPHAR\\SebastianBergmann\\Diff\\Differ".into())
        );
    }

    #[test]
    fn extracts_humbug_prefixed_name() {
        assert_eq!(
            extract_prefixed_name(HUMBUG_MSG),
            Some("_HumbugBox456\\Composer\\Autoload\\ClassLoader".into())
        );
    }

    #[test]
    fn prefixed_name_returns_none_without_marker() {
        assert_eq!(extract_prefixed_name("Some unrelated error."), None);
    }

    // ── extract_corrected_name ──────────────────────────────────────

    #[test]
    fn extracts_phpstan_corrected_name() {
        assert_eq!(
            extract_corrected_name(PHPSTAN_MSG),
            Some("\\SomeClass".into())
        );
    }

    #[test]
    fn extracts_rector_corrected_name() {
        assert_eq!(
            extract_corrected_name(RECTOR_MSG),
            Some("\\Symfony\\Component\\Console\\Application".into())
        );
    }

    #[test]
    fn corrected_name_returns_none_without_tip() {
        assert_eq!(
            extract_corrected_name("Referencing prefixed PHPStan class: _PHPStan_test\\Bar."),
            None
        );
    }

    #[test]
    fn corrected_name_returns_none_for_wrong_tip_format() {
        let msg = "Some error.\nSome tip without the expected format.";
        assert_eq!(extract_corrected_name(msg), None);
    }

    #[test]
    fn handles_extra_whitespace_in_tip() {
        let msg = "Error.\n  Did you mean to type  \\Foo ?";
        assert_eq!(extract_corrected_name(msg), Some("\\Foo".into()));
    }

    // ── parse_prefixed_diagnostic ───────────────────────────────────

    #[test]
    fn parses_full_diagnostic_phpstan() {
        let info = parse_prefixed_diagnostic(PHPSTAN_MSG).unwrap();
        assert_eq!(info.prefixed, "_PHPStan_test\\SomeClass");
        assert_eq!(info.corrected, "\\SomeClass");
    }

    #[test]
    fn parses_full_diagnostic_rector() {
        let info = parse_prefixed_diagnostic(RECTOR_MSG).unwrap();
        assert_eq!(
            info.prefixed,
            "RectorPrefix202501\\Symfony\\Component\\Console\\Application"
        );
        assert_eq!(info.corrected, "\\Symfony\\Component\\Console\\Application");
    }

    #[test]
    fn parses_full_diagnostic_scoper() {
        let info = parse_prefixed_diagnostic(SCOPER_MSG).unwrap();
        assert_eq!(info.prefixed, "_PhpScoper123\\GuzzleHttp\\Client");
        assert_eq!(info.corrected, "\\GuzzleHttp\\Client");
    }

    #[test]
    fn returns_none_without_both_parts() {
        // No tip → can't get corrected name.
        assert!(
            parse_prefixed_diagnostic("Referencing prefixed PHPStan class: _PHPStan_test\\Bar.")
                .is_none()
        );
    }

    // ── find_occurrence ─────────────────────────────────────────────

    #[test]
    fn finds_prefixed_at_start_of_line() {
        assert_eq!(
            find_occurrence(
                "_PHPStan_test\\SomeClass::create()",
                "_PHPStan_test\\SomeClass"
            ),
            Some(0)
        );
    }

    #[test]
    fn finds_prefixed_after_new() {
        assert_eq!(
            find_occurrence("new _PHPStan_test\\SomeClass()", "_PHPStan_test\\SomeClass"),
            Some(4)
        );
    }

    #[test]
    fn finds_fqn_prefixed_after_new() {
        assert_eq!(
            find_occurrence(
                "new \\_PHPStan_test\\SomeClass()",
                "\\_PHPStan_test\\SomeClass"
            ),
            Some(4)
        );
    }

    #[test]
    fn skips_partial_match_longer_name() {
        assert_eq!(
            find_occurrence(
                "new _PHPStan_test\\SomeClassFactory()",
                "_PHPStan_test\\SomeClass"
            ),
            None
        );
    }

    #[test]
    fn skips_embedded_in_longer_prefix() {
        // The name preceded by an identifier char should not match.
        assert_eq!(
            find_occurrence(
                "x_PHPStan_test\\SomeClass::bar()",
                "_PHPStan_test\\SomeClass"
            ),
            None
        );
    }

    #[test]
    fn finds_prefixed_before_semicolon() {
        assert_eq!(
            find_occurrence(
                "$x = new _PhpScoper123\\GuzzleHttp\\Client;",
                "_PhpScoper123\\GuzzleHttp\\Client"
            ),
            Some(9)
        );
    }

    #[test]
    fn finds_fqn_prefixed_before_semicolon() {
        assert_eq!(
            find_occurrence(
                "$x = new \\_PhpScoper123\\GuzzleHttp\\Client;",
                "\\_PhpScoper123\\GuzzleHttp\\Client"
            ),
            Some(9)
        );
    }

    #[test]
    fn finds_prefixed_before_paren() {
        assert_eq!(
            find_occurrence(
                "_PhpScoper123\\GuzzleHttp\\Client::create()",
                "_PhpScoper123\\GuzzleHttp\\Client"
            ),
            Some(0)
        );
    }

    #[test]
    fn allows_backslash_after_match() {
        // After the match, a `\` could indicate a deeper namespace.
        // This is actually a longer name, so it should NOT match.
        // `_PHPStan_test\Some` should not match in `_PHPStan_test\SomeClass`.
        assert_eq!(
            find_occurrence("new _PHPStan_test\\SomeClass()", "_PHPStan_test\\Some"),
            None
        );
    }

    // ── build_fix_prefixed_edit ─────────────────────────────────────

    #[test]
    fn builds_edit_phpstan_class_bare() {
        let content = "<?php\nnew _PHPStan_test\\SomeClass();";
        let edit =
            build_fix_prefixed_edit(content, 1, "_PHPStan_test\\SomeClass", "\\SomeClass").unwrap();

        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.range.start.character, 4);
        // len("_PHPStan_test\SomeClass") = 23, 4+23=27
        assert_eq!(edit.range.end.character, 27);
        assert_eq!(edit.new_text, "\\SomeClass");
    }

    #[test]
    fn builds_edit_phpstan_class_fqn() {
        let content = "<?php\nnew \\_PHPStan_test\\SomeClass();";
        let edit = build_fix_prefixed_edit(content, 1, "\\_PHPStan_test\\SomeClass", "\\SomeClass")
            .unwrap();

        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.range.start.character, 4);
        // len("\_PHPStan_test\SomeClass") = 24, 4+24=28
        assert_eq!(edit.range.end.character, 28);
        assert_eq!(edit.new_text, "\\SomeClass");
    }

    #[test]
    fn builds_edit_rector_class() {
        let content = "<?php\nRectorPrefix202501\\Symfony\\Component\\Console\\Application::run();";
        let edit = build_fix_prefixed_edit(
            content,
            1,
            "RectorPrefix202501\\Symfony\\Component\\Console\\Application",
            "\\Symfony\\Component\\Console\\Application",
        )
        .unwrap();

        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.range.start.character, 0);
        assert_eq!(edit.new_text, "\\Symfony\\Component\\Console\\Application");
    }

    #[test]
    fn returns_none_when_prefixed_not_found() {
        let content = "<?php\nnew \\SomeClass();";
        assert!(
            build_fix_prefixed_edit(content, 1, "_PHPStan_test\\SomeClass", "\\SomeClass")
                .is_none()
        );
    }

    #[test]
    fn returns_none_for_out_of_bounds_line() {
        let content = "<?php\n";
        assert!(
            build_fix_prefixed_edit(content, 5, "_PHPStan_test\\SomeClass", "\\SomeClass")
                .is_none()
        );
    }

    // ── byte_offset_to_utf16 ───────────────────────────────────────

    #[test]
    fn utf16_ascii_line() {
        assert_eq!(byte_offset_to_utf16("new _PHPStan_foo\\SomeClass()", 4), 4);
        assert_eq!(
            byte_offset_to_utf16("new _PHPStan_foo\\SomeClass()", 25),
            25
        );
    }

    #[test]
    fn utf16_with_multibyte_before() {
        // "é" is 2 bytes in UTF-8 but 1 UTF-16 code unit.
        let line = "é_PHPStan_test\\Cls";
        assert_eq!(byte_offset_to_utf16(line, 2), 1); // after "é"
    }

    // ── is_fix_prefixed_class_stale ────────────────────────────────

    #[test]
    fn stale_when_prefix_removed() {
        let content = "<?php\nnew \\SomeClass();";
        assert!(is_fix_prefixed_class_stale(content, 1, PHPSTAN_MSG));
    }

    #[test]
    fn not_stale_when_prefix_still_present() {
        let content = "<?php\nnew _PHPStan_test\\SomeClass();";
        assert!(!is_fix_prefixed_class_stale(content, 1, PHPSTAN_MSG));
    }

    #[test]
    fn not_stale_when_fqn_prefix_still_present() {
        let content = "<?php\nnew \\_PHPStan_test\\SomeClass();";
        assert!(!is_fix_prefixed_class_stale(content, 1, PHPSTAN_MSG));
    }

    #[test]
    fn stale_when_line_deleted() {
        let content = "<?php\n";
        assert!(is_fix_prefixed_class_stale(content, 5, PHPSTAN_MSG));
    }

    #[test]
    fn not_stale_without_parseable_message() {
        let content = "<?php\nnew _PHPStan_foo\\SomeClass();";
        // Cannot determine staleness without the prefixed name, so
        // conservatively report as not stale.
        assert!(!is_fix_prefixed_class_stale(
            content,
            1,
            "Some error without the expected format."
        ));
    }

    // ── Full roundtrip tests ────────────────────────────────────────

    #[test]
    fn full_roundtrip_phpstan_bare() {
        let content = "<?php\nnew _PHPStan_test\\SomeClass();";

        // Parse diagnostic.
        let info = parse_prefixed_diagnostic(PHPSTAN_MSG).unwrap();
        assert_eq!(info.prefixed, "_PHPStan_test\\SomeClass");
        assert_eq!(info.corrected, "\\SomeClass");

        // Build edit using the bare prefixed name.
        let edit = build_fix_prefixed_edit(content, 1, &info.prefixed, &info.corrected).unwrap();
        assert_eq!(edit.new_text, "\\SomeClass");

        // Apply edit and verify result.
        let line = content.lines().nth(1).unwrap();
        let start = edit.range.start.character as usize;
        let end = edit.range.end.character as usize;
        let before = &line[..start];
        let after = &line[end..];
        let result = format!("<?php\n{}{}{}", before, edit.new_text, after);
        assert_eq!(result, "<?php\nnew \\SomeClass();");

        // Stale detection: original content is not stale.
        assert!(!is_fix_prefixed_class_stale(content, 1, PHPSTAN_MSG));

        // After fix, content is stale.
        assert!(is_fix_prefixed_class_stale(&result, 1, PHPSTAN_MSG));
    }

    #[test]
    fn full_roundtrip_phpstan_fqn() {
        // In namespace context, PHP uses `\_PHPStan_test\SomeClass`
        // (with leading backslash).  PHPStan's message still says
        // `_PHPStan_test\SomeClass` without the leading backslash.
        let content = "<?php\nnew \\_PHPStan_test\\SomeClass();";

        let info = parse_prefixed_diagnostic(PHPSTAN_MSG).unwrap();

        // The FQN form includes the leading backslash.
        let fqn_prefixed = format!("\\{}", info.prefixed);
        let edit = build_fix_prefixed_edit(content, 1, &fqn_prefixed, &info.corrected).unwrap();
        assert_eq!(edit.new_text, "\\SomeClass");

        // Apply edit and verify result.
        let line = content.lines().nth(1).unwrap();
        let start = edit.range.start.character as usize;
        let end = edit.range.end.character as usize;
        let before = &line[..start];
        let after = &line[end..];
        let result = format!("<?php\n{}{}{}", before, edit.new_text, after);
        assert_eq!(result, "<?php\nnew \\SomeClass();");

        // Stale detection.
        assert!(!is_fix_prefixed_class_stale(content, 1, PHPSTAN_MSG));
        assert!(is_fix_prefixed_class_stale(&result, 1, PHPSTAN_MSG));
    }

    #[test]
    fn full_roundtrip_rector_bare() {
        let content = "<?php\nRectorPrefix202501\\Symfony\\Component\\Console\\Application::run();";

        let info = parse_prefixed_diagnostic(RECTOR_MSG).unwrap();
        let edit = build_fix_prefixed_edit(content, 1, &info.prefixed, &info.corrected).unwrap();

        let line = content.lines().nth(1).unwrap();
        let start = edit.range.start.character as usize;
        let end = edit.range.end.character as usize;
        let before = &line[..start];
        let after = &line[end..];
        let result = format!("<?php\n{}{}{}", before, edit.new_text, after);
        assert_eq!(
            result,
            "<?php\n\\Symfony\\Component\\Console\\Application::run();"
        );
    }

    #[test]
    fn full_roundtrip_scoper() {
        let content = "<?php\n$c = new _PhpScoper123\\GuzzleHttp\\Client();";

        let info = parse_prefixed_diagnostic(SCOPER_MSG).unwrap();
        let edit = build_fix_prefixed_edit(content, 1, &info.prefixed, &info.corrected).unwrap();
        assert_eq!(edit.new_text, "\\GuzzleHttp\\Client");
    }

    #[test]
    fn full_roundtrip_phpunit() {
        let content = "<?php\n$d = new PHPUnitPHAR\\SebastianBergmann\\Diff\\Differ();";

        let info = parse_prefixed_diagnostic(PHPUNIT_MSG).unwrap();
        let edit = build_fix_prefixed_edit(content, 1, &info.prefixed, &info.corrected).unwrap();
        assert_eq!(edit.new_text, "\\SebastianBergmann\\Diff\\Differ");
    }

    #[test]
    fn full_roundtrip_humbug() {
        let content = "<?php\n$e = new _HumbugBox456\\Composer\\Autoload\\ClassLoader();";

        let info = parse_prefixed_diagnostic(HUMBUG_MSG).unwrap();
        let edit = build_fix_prefixed_edit(content, 1, &info.prefixed, &info.corrected).unwrap();
        assert_eq!(edit.new_text, "\\Composer\\Autoload\\ClassLoader");
    }

    #[test]
    fn multiple_occurrences_fixes_first() {
        // When there are multiple occurrences on the same line, fix the
        // first one.  PHPStan reports a separate diagnostic for each,
        // so each gets its own code action.
        let content = "<?php\n_PHPStan_test\\SomeClass::bar(new _PHPStan_test\\SomeClass());";

        let edit =
            build_fix_prefixed_edit(content, 1, "_PHPStan_test\\SomeClass", "\\SomeClass").unwrap();
        assert_eq!(edit.range.start.character, 0);
    }

    #[test]
    fn does_not_match_substring_class() {
        // If the line has `_PHPStan_foo\SomeClassFactory` but the
        // prefixed name is `_PHPStan_foo\SomeClass`, we should not match.
        let content = "<?php\nnew _PHPStan_test\\SomeClassFactory();";

        assert!(
            build_fix_prefixed_edit(content, 1, "_PHPStan_test\\SomeClass", "\\SomeClass")
                .is_none()
        );
    }

    #[test]
    fn instanceof_context() {
        let content = "<?php\nif ($a instanceof _PHPStan_foo\\SomeInterface) {}";
        let edit =
            build_fix_prefixed_edit(content, 1, "_PHPStan_foo\\SomeInterface", "\\SomeInterface")
                .unwrap();
        assert_eq!(edit.new_text, "\\SomeInterface");
    }

    #[test]
    fn type_hint_context() {
        let content = "<?php\nfunction doSomething(_PhpScoper123\\GuzzleHttp\\ClientInterface $client): void {}";
        let edit = build_fix_prefixed_edit(
            content,
            1,
            "_PhpScoper123\\GuzzleHttp\\ClientInterface",
            "\\GuzzleHttp\\ClientInterface",
        )
        .unwrap();
        assert_eq!(edit.new_text, "\\GuzzleHttp\\ClientInterface");
    }

    #[test]
    fn catch_clause_context() {
        let content = "<?php\n} catch (_PHPStan_test\\SomeException $e) {";
        let edit = build_fix_prefixed_edit(
            content,
            1,
            "_PHPStan_test\\SomeException",
            "\\SomeException",
        )
        .unwrap();
        assert_eq!(edit.new_text, "\\SomeException");
    }
}
