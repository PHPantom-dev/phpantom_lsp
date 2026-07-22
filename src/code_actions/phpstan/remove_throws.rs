//! "Remove @throws" code action for PHPStan `throws.unusedType` and
//! `throws.notThrowable`.
//!
//! When PHPStan reports that a `@throws` tag references a type that is
//! either not thrown (`throws.unusedType`) or not a subtype of
//! `Throwable` (`throws.notThrowable`), this code action offers to
//! remove the offending `@throws` line from the docblock.
//!
//! After applying the edit the triggering diagnostic is eagerly removed
//! from the PHPStan cache so the user gets instant visual feedback
//! without waiting for the next PHPStan run.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `throws.unusedType` or `throws.notThrowable` overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::CodeActionData;
use crate::code_actions::make_code_action_data;
use crate::php_type::PhpType;
use crate::util::{offset_to_position, ranges_overlap, short_name};

/// PHPStan identifiers we match on.
const UNUSED_TYPE_ID: &str = "throws.unusedType";
const NOT_THROWABLE_ID: &str = "throws.notThrowable";

impl Backend {
    /// Collect "Remove @throws" code actions for PHPStan
    /// `throws.unusedType` and `throws.notThrowable` diagnostics.
    pub(crate) fn collect_remove_throws_actions(
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

            if identifier != UNUSED_TYPE_ID && identifier != NOT_THROWABLE_ID {
                continue;
            }

            // Extract the type name from the message (validation only).
            let type_name = match extract_throws_type(&diag.message, identifier) {
                Some(t) => t,
                None => continue,
            };

            let type_name_str = type_name.to_string();
            let short_name = short_name(&type_name_str);

            // Find the docblock above the diagnostic line (validation only).
            let diag_line = diag.range.start.line as usize;
            let docblock = match find_docblock_above_line(content, diag_line) {
                Some(db) => db,
                None => continue,
            };

            // Validate that the @throws line can be removed (validation only).
            if build_remove_throws_edit(
                content,
                &docblock,
                &type_name_str,
                &file_use_map,
                &file_namespace,
            )
            .is_none()
            {
                continue;
            }

            let title = format!("Remove @throws {}", short_name);

            let extra = serde_json::json!({
                "diagnostic_message": diag.message,
                "diagnostic_line": diag_line,
                "diagnostic_code": identifier,
            });

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(make_code_action_data(
                    "phpstan.removeThrows",
                    uri,
                    &params.range,
                    extra,
                )),
            }));
        }
    }

    /// Resolve a "Remove @throws" code action by recomputing the
    /// workspace edit from the stored data.
    pub(crate) fn resolve_remove_throws(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let extra = &data.extra;
        let message = extra.get("diagnostic_message")?.as_str()?;
        let line = extra.get("diagnostic_line")?.as_u64()? as usize;
        let code = extra.get("diagnostic_code")?.as_str()?;

        let type_name = extract_throws_type(message, code)?;
        let type_name_str = type_name.to_string();

        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        let docblock = find_docblock_above_line(content, line)?;
        let throws_edit = build_remove_throws_edit(
            content,
            &docblock,
            &type_name_str,
            &file_use_map,
            &file_namespace,
        )?;

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, vec![throws_edit]);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract the type from a PHPStan `throws.unusedType` or
/// `throws.notThrowable` message as a [`PhpType`].
///
/// `throws.unusedType` formats:
/// - `"Method Ns\Cls::method() has Ns\Ex in PHPDoc @throws tag but it's not thrown."`
/// - `"Function foo() has Ns\Ex in PHPDoc @throws tag but it's not thrown."`
/// - `"Get hook for property Ns\Cls::$prop has Ns\Ex in PHPDoc @throws tag but it's not thrown."`
///
/// `throws.notThrowable` format:
/// - `"PHPDoc tag @throws with type Ns\Ex is not subtype of Throwable"`
fn extract_throws_type(message: &str, identifier: &str) -> Option<PhpType> {
    let raw = if identifier == UNUSED_TYPE_ID {
        // Pattern: "has <type> in PHPDoc @throws tag"
        let marker = " has ";
        let start = message.find(marker)? + marker.len();
        let rest = &message[start..];
        let end = rest.find(" in PHPDoc @throws tag")?;
        rest[..end].trim()
    } else {
        // Pattern: "@throws with type <type> is not subtype of Throwable"
        let marker = "@throws with type ";
        let start = message.find(marker)? + marker.len();
        let rest = &message[start..];
        let end = rest.find(" is not subtype")?;
        rest[..end].trim()
    };

    if raw.is_empty() {
        return None;
    }

    Some(PhpType::parse(raw))
}

/// Information about a docblock found above a given line.
struct DocblockAbove {
    /// Byte offset of the `/**`.
    start: usize,
    /// Byte offset just past the `*/`.
    end: usize,
    /// The raw docblock text.
    text: String,
}

/// Find the docblock immediately above the given line.
///
/// The diagnostic line is the method/function signature.  The docblock
/// (if any) sits directly above it, possibly separated by blank lines
/// or attribute lines.
fn find_docblock_above_line(content: &str, line: usize) -> Option<DocblockAbove> {
    let lines: Vec<&str> = content.lines().collect();
    if line == 0 || line > lines.len() {
        return None;
    }

    // Walk backward from the line before the diagnostic to find `*/`.
    let mut doc_end_line = None;
    for i in (0..line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.ends_with("*/") {
            doc_end_line = Some(i);
            break;
        }
        // Attributes (#[...]) can appear between docblock and function.
        if trimmed.starts_with("#[") {
            continue;
        }
        // Anything else means no docblock.
        break;
    }

    let end_line = doc_end_line?;

    // Walk backward from end_line to find `/**`.
    let mut doc_start_line = None;
    for i in (0..=end_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.contains("/**") {
            doc_start_line = Some(i);
            break;
        }
        // Should be a `*`-prefixed line.
        if !trimmed.starts_with('*') && !trimmed.ends_with("*/") {
            break;
        }
    }

    let start_line = doc_start_line?;

    // Convert line numbers to byte offsets.
    let mut byte_offset = 0;
    let mut start_byte = 0;
    let mut end_byte = 0;
    for (i, line_text) in lines.iter().enumerate() {
        if i == start_line {
            start_byte = byte_offset;
        }
        byte_offset += line_text.len() + 1; // +1 for newline
        if i == end_line {
            end_byte = byte_offset; // include trailing newline
        }
    }

    // Trim to just the docblock including its indentation.
    let text = content
        .get(start_byte..end_byte.min(content.len()))
        .unwrap_or("")
        .to_string();

    Some(DocblockAbove {
        start: start_byte,
        end: end_byte.min(content.len()),
        text,
    })
}

/// Build a `TextEdit` that removes the `@throws` line matching the
/// given type from the docblock.
///
/// If the docblock would become "empty" (only summary or nothing) after
/// removal, this still preserves the docblock shell — removing the
/// whole docblock could lose `@param` / `@return` / summary text.
fn build_remove_throws_edit(
    content: &str,
    docblock: &DocblockAbove,
    type_name: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> Option<TextEdit> {
    // PHPStan reports FQNs already (e.g. `App\Http\Controllers\Ex`),
    // so just normalise — don't resolve through the use-map.
    let target_fqn = type_name
        .strip_prefix('\\')
        .unwrap_or(type_name)
        .to_lowercase();

    // Find the @throws line(s) to remove.
    let doc_lines: Vec<&str> = docblock.text.lines().collect();

    // Identify which line indices to remove.
    let mut lines_to_remove: Vec<usize> = Vec::new();
    for (i, line) in doc_lines.iter().enumerate() {
        let mut trimmed = line.trim();
        // Strip docblock delimiters so single-line `/** @throws ... */`
        // and multi-line `* @throws ...` are both handled.
        if let Some(inner) = trimmed.strip_prefix("/**") {
            trimmed = inner.trim_start();
        }
        if let Some(inner) = trimmed.strip_suffix("*/") {
            trimmed = inner.trim_end();
        }
        trimmed = trimmed.trim_start_matches('*').trim();
        if let Some(rest) = trimmed.strip_prefix("@throws") {
            let rest = rest.trim_start();
            let tag_type = rest.split_whitespace().next().unwrap_or("");
            let tag_fqn =
                crate::util::resolve_to_fqn(tag_type, use_map, file_namespace).to_lowercase();

            if tag_fqn == target_fqn {
                lines_to_remove.push(i);
            }
        }
    }

    if lines_to_remove.is_empty() {
        return None;
    }

    // Also remove blank `*` separator lines that would be orphaned.
    // If the line before a removed line is a blank `*`, and the line
    // after the removed block is `*/` or another blank `*`, remove
    // the blank line too.
    let mut extra_removals: Vec<usize> = Vec::new();
    for &idx in &lines_to_remove {
        // Check the line before.
        if idx > 0 && !lines_to_remove.contains(&(idx - 1)) {
            let prev = doc_lines[idx - 1].trim().trim_start_matches('*').trim();
            if prev.is_empty() {
                // Check what follows the removed block.
                let next_idx = lines_to_remove
                    .iter()
                    .filter(|&&j| j > idx)
                    .max()
                    .copied()
                    .unwrap_or(idx)
                    + 1;
                if next_idx < doc_lines.len() {
                    let next = doc_lines[next_idx].trim();
                    if next == "*/" || next.trim_start_matches('*').trim().is_empty() {
                        extra_removals.push(idx - 1);
                    }
                } else {
                    extra_removals.push(idx - 1);
                }
            }
        }
    }
    lines_to_remove.extend(extra_removals);
    lines_to_remove.sort();
    lines_to_remove.dedup();

    // Build the new docblock text by excluding the removed lines.
    let new_lines: Vec<&str> = doc_lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !lines_to_remove.contains(i))
        .map(|(_, l)| *l)
        .collect();

    // If the docblock is now essentially empty (just `/**` and `*/`
    // with maybe blank `*` lines), keep it minimal.
    let has_content = new_lines.iter().any(|l| {
        let mut t = l.trim();
        if let Some(inner) = t.strip_prefix("/**") {
            t = inner.trim_start();
        }
        if let Some(inner) = t.strip_suffix("*/") {
            t = inner.trim_end();
        }
        t = t.trim_start_matches('*').trim();
        !t.is_empty()
    });

    let new_text = if !has_content && new_lines.len() <= 3 {
        // The docblock is essentially empty after removal — remove it
        // entirely (including its trailing newline).
        String::new()
    } else {
        let mut text = new_lines.join("\n");
        // Preserve trailing newline if original had one.
        if docblock.text.ends_with('\n') && !text.ends_with('\n') {
            text.push('\n');
        }
        text
    };

    let start = offset_to_position(content, docblock.start);
    let end = offset_to_position(content, docblock.end);

    Some(TextEdit {
        range: Range { start, end },
        new_text,
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_throws_type ─────────────────────────────────────────

    #[test]
    fn extracts_unused_type_from_method_message() {
        let msg = "Method App\\Controllers\\Foo::bar() has Acme\\Decimal\\Decimal in PHPDoc @throws tag but it's not thrown.";
        let t = extract_throws_type(msg, UNUSED_TYPE_ID).unwrap();
        assert_eq!(t.to_string(), "Acme\\Decimal\\Decimal");
    }

    #[test]
    fn extracts_unused_type_from_function_message() {
        let msg = "Function doStuff() has App\\Exceptions\\FooException in PHPDoc @throws tag but it's not thrown.";
        let t = extract_throws_type(msg, UNUSED_TYPE_ID).unwrap();
        assert_eq!(t.to_string(), "App\\Exceptions\\FooException");
    }

    #[test]
    fn extracts_unused_type_from_property_hook_message() {
        let msg = "Get hook for property App\\Foo::$bar has App\\Exceptions\\PropException in PHPDoc @throws tag but it's not thrown.";
        let t = extract_throws_type(msg, UNUSED_TYPE_ID).unwrap();
        assert_eq!(t.to_string(), "App\\Exceptions\\PropException");
    }

    #[test]
    fn extracts_not_throwable_type() {
        let msg =
            "PHPDoc tag @throws with type App\\Http\\Controllers\\not is not subtype of Throwable";
        let t = extract_throws_type(msg, NOT_THROWABLE_ID).unwrap();
        assert_eq!(t.to_string(), "App\\Http\\Controllers\\not");
    }

    #[test]
    fn extracts_not_throwable_fqn_type() {
        let msg = "PHPDoc tag @throws with type \\TheSeer\\Tokenizer\\Exception is not subtype of Throwable";
        let t = extract_throws_type(msg, NOT_THROWABLE_ID).unwrap();
        assert_eq!(t.to_string(), "\\TheSeer\\Tokenizer\\Exception");
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        assert!(extract_throws_type("Some other error.", UNUSED_TYPE_ID).is_none());
        assert!(extract_throws_type("Some other error.", NOT_THROWABLE_ID).is_none());
    }

    // ── short_name_from_type ────────────────────────────────────────

    #[test]
    fn short_name_simple() {
        assert_eq!(short_name("RuntimeException"), "RuntimeException");
    }

    #[test]
    fn short_name_namespaced() {
        assert_eq!(short_name("App\\Exceptions\\FooException"), "FooException");
    }

    #[test]
    fn short_name_leading_backslash() {
        assert_eq!(short_name("\\TheSeer\\Tokenizer\\Exception"), "Exception");
    }

    #[test]
    fn short_name_non_class() {
        // "not even correct" — the short name is the whole thing
        // since there are no backslashes.
        assert_eq!(short_name("not"), "not");
    }

    // ── find_docblock_above_line ────────────────────────────────────

    #[test]
    fn finds_docblock_directly_above() {
        let php = "\
<?php
class Foo {
    /**
     * @throws Decimal
     */
    public function bar(): void {}
}
";
        // Line 5 is `    public function bar(): void {}`
        let db = find_docblock_above_line(php, 5).unwrap();
        assert!(db.text.contains("@throws Decimal"), "got: {}", db.text);
    }

    #[test]
    fn finds_docblock_with_blank_line_between() {
        // Some code styles leave a blank line between docblock and
        // attributes/function.  We skip blank lines.
        let php = "\
<?php
class Foo {
    /**
     * @throws Decimal
     */

    public function bar(): void {}
}
";
        let db = find_docblock_above_line(php, 6).unwrap();
        assert!(db.text.contains("@throws Decimal"), "got: {}", db.text);
    }

    #[test]
    fn finds_docblock_with_attribute_between() {
        let php = "\
<?php
class Foo {
    /**
     * @throws Decimal
     */
    #[SomeAttribute]
    public function bar(): void {}
}
";
        let db = find_docblock_above_line(php, 6).unwrap();
        assert!(db.text.contains("@throws Decimal"), "got: {}", db.text);
    }

    #[test]
    fn no_docblock_found() {
        let php = "\
<?php
class Foo {
    public function bar(): void {}
}
";
        assert!(find_docblock_above_line(php, 2).is_none());
    }

    // ── build_remove_throws_edit ────────────────────────────────────

    #[test]
    fn removes_throws_line_from_docblock() {
        let php = "\
<?php
class Foo {
    /**
     * Summary.
     *
     * @throws Decimal
     */
    public function bar(): void {}
}
";
        let use_map: HashMap<String, String> =
            HashMap::from([("Decimal".to_string(), "Acme\\Decimal\\Decimal".to_string())]);
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 7).unwrap();
        let edit =
            build_remove_throws_edit(php, &db, "Acme\\Decimal\\Decimal", &use_map, &ns).unwrap();
        assert!(
            !edit.new_text.contains("@throws"),
            "should remove @throws: {:?}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("Summary."),
            "should preserve summary: {:?}",
            edit.new_text
        );
    }

    #[test]
    fn removes_fqn_throws_line() {
        let php = "\
<?php
class Foo {
    /**
     * @throws \\TheSeer\\Tokenizer\\Exception
     */
    public function bar(): void {}
}
";
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 5).unwrap();
        let edit =
            build_remove_throws_edit(php, &db, "\\TheSeer\\Tokenizer\\Exception", &use_map, &ns)
                .unwrap();
        // The docblock only had the @throws line, so the entire
        // docblock should be removed.
        assert_eq!(
            edit.new_text, "",
            "empty docblock should be removed entirely"
        );
    }

    #[test]
    fn removes_short_name_throws_matching_fqn() {
        let php = "\
<?php
class Foo {
    /**
     * @throws Exception
     */
    public function bar(): void {}
}
";
        // The docblock says `Exception` (unqualified). With no use-map
        // and no namespace the resolver keeps it as `Exception`, which
        // differs from the FQN `TheSeer\Tokenizer\Exception`. To make
        // the match work the use-map must map `Exception` to the FQN.
        let use_map: HashMap<String, String> = HashMap::from([(
            "Exception".to_string(),
            "TheSeer\\Tokenizer\\Exception".to_string(),
        )]);
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 5).unwrap();
        let edit =
            build_remove_throws_edit(php, &db, "TheSeer\\Tokenizer\\Exception", &use_map, &ns)
                .unwrap();
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn preserves_other_throws_tags() {
        let php = "\
<?php
class Foo {
    /**
     * @throws FooException
     * @throws BarException
     */
    public function bar(): void {}
}
";
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 6).unwrap();
        let edit = build_remove_throws_edit(php, &db, "FooException", &use_map, &ns).unwrap();
        assert!(
            !edit.new_text.contains("FooException"),
            "should remove FooException: {:?}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("@throws BarException"),
            "should keep BarException: {:?}",
            edit.new_text
        );
    }

    #[test]
    fn removes_throws_with_non_class_text() {
        // "not even correct" — PHPStan reports the parsed type which
        // is just "not".
        let php = "\
<?php
class Foo {
    /**
     * @throws not even correct
     */
    public function bar(): void {}
}
";
        let db = find_docblock_above_line(php, 5).unwrap();
        // PHPStan reports the type as "App\Http\Controllers\not"
        // because it resolves relative to the namespace. The file is
        // in that namespace, so `not` resolves to the same FQN.
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = Some("App\\Http\\Controllers".to_string());
        let edit = build_remove_throws_edit(php, &db, "App\\Http\\Controllers\\not", &use_map, &ns)
            .unwrap();
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn removes_entire_empty_docblock() {
        let php = "\
<?php
class Foo {
    /**
     * @throws Decimal
     */
    public function bar(): void {}
}
";
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 5).unwrap();
        let edit = build_remove_throws_edit(php, &db, "Decimal", &use_map, &ns).unwrap();
        assert_eq!(
            edit.new_text, "",
            "docblock with only @throws should be removed"
        );
    }

    #[test]
    fn keeps_docblock_with_other_content() {
        let php = "\
<?php
class Foo {
    /**
     * Summary.
     *
     * @param string $a
     * @throws Decimal
     *
     * @return string
     */
    public function bar(string $a): string {}
}
";
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 10).unwrap();
        let edit = build_remove_throws_edit(php, &db, "Decimal", &use_map, &ns).unwrap();
        assert!(
            edit.new_text.contains("Summary."),
            "should keep summary: {:?}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("@param"),
            "should keep @param: {:?}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("@return"),
            "should keep @return: {:?}",
            edit.new_text
        );
        assert!(
            !edit.new_text.contains("@throws"),
            "should remove @throws: {:?}",
            edit.new_text
        );
    }

    #[test]
    fn no_match_returns_none() {
        let php = "\
<?php
class Foo {
    /**
     * @throws FooException
     */
    public function bar(): void {}
}
";
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 5).unwrap();
        assert!(build_remove_throws_edit(php, &db, "BarException", &use_map, &ns).is_none());
    }

    #[test]
    fn removes_orphaned_blank_separator() {
        let php = "\
<?php
class Foo {
    /**
     * @param string $a
     *
     * @throws Decimal
     */
    public function bar(string $a): void {}
}
";
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = None;
        let db = find_docblock_above_line(php, 7).unwrap();
        let edit = build_remove_throws_edit(php, &db, "Decimal", &use_map, &ns).unwrap();
        // The blank `*` line between @param and @throws should also
        // be removed so we don't get a trailing blank line before `*/`.
        let trailing_blank = edit.new_text.contains(" *\n     */");
        assert!(
            !trailing_blank,
            "should not leave orphaned blank line: {:?}",
            edit.new_text
        );
    }

    // ── Single-line docblock ────────────────────────────────────────

    #[test]
    fn removes_single_line_docblock_with_throws() {
        let php = "<?php\nclass Foo {\n    /** @throws Decimal */\n    public function bar(): void {}\n}\n";
        let use_map: HashMap<String, String> = HashMap::new();
        let ns: Option<String> = None;
        // Line 3 is `    public function bar(): void {}`
        let db = find_docblock_above_line(php, 3)
            .expect("should find single-line docblock above line 3");
        let edit = build_remove_throws_edit(php, &db, "Decimal", &use_map, &ns).unwrap();
        assert_eq!(
            edit.new_text, "",
            "single-line docblock with only @throws should be removed"
        );
    }
}
