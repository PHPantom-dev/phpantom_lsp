//! Output formatting for the `analyze` command.
//!
//! Renders collected diagnostics in the three supported formats:
//! the PHPStan-style table (default), GitHub Actions workflow
//! annotations, and JSON. Also owns the progress bar drawn during the
//! diagnostic pass.

use std::collections::BTreeMap;

use serde::Serialize;
use tower_lsp::lsp_types::*;

use super::{FileDiagnostic, OutputFormat};

// ── GitHub Actions annotations ──────────────────────────────────────────────

/// Emit GitHub Actions workflow commands for all diagnostics.
///
/// Each diagnostic is printed as a `::error` or `::warning` line so that
/// GitHub Actions surfaces them as inline annotations on pull request diffs.
/// See: <https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/workflow-commands-for-github-actions>
pub(super) fn print_github_annotations(file_diagnostics: &[(String, Vec<FileDiagnostic>)]) {
    for (path, diagnostics) in file_diagnostics {
        for diag in diagnostics {
            let level = match diag.severity {
                DiagnosticSeverity::ERROR | DiagnosticSeverity::WARNING => {
                    severity_name(diag.severity)
                }
                _ => "notice",
            };
            let title = diag.identifier.as_deref().unwrap_or("");
            println!(
                "{}",
                github_annotation(level, path, diag.line, title, &diag.message)
            );
        }
    }
}

/// One GitHub Actions workflow command annotating `file` at `line` with
/// `message`, at `level` (`error`, `warning`, or `notice`).
///
/// An empty `title` is left out rather than written as `title=`.
pub(crate) fn github_annotation(
    level: &str,
    file: &str,
    line: impl std::fmt::Display,
    title: &str,
    message: &str,
) -> String {
    let message = format_github_message(message);
    if title.is_empty() {
        format!("::{level} file={file},line={line},col=0::{message}")
    } else {
        format!("::{level} file={file},line={line},col=0,title={title}::{message}")
    }
}

/// Format a message for GitHub Actions workflow commands.
///
/// Newlines are encoded as `%0A` per the GitHub Actions spec, and `@mentions`
/// are wrapped in backticks to prevent GitHub from sending notifications
/// (matching PHPStan's `GithubErrorFormatter` behaviour).
pub(crate) fn format_github_message(message: &str) -> String {
    let message = message.replace('\n', "%0A");
    let mut result = String::with_capacity(message.len());
    let mut chars = message.char_indices().peekable();
    let mut last_end = 0;
    while let Some((i, c)) = chars.next() {
        if c == '@' {
            let before_is_space = i == 0
                || message
                    .as_bytes()
                    .get(i - 1)
                    .is_none_or(|b| b.is_ascii_whitespace());
            if before_is_space {
                // Collect the mention: @[a-zA-Z0-9_-]+
                let start = i + 1;
                let mut end = start;
                while let Some(&(j, nc)) = chars.peek() {
                    if nc.is_ascii_alphanumeric() || nc == '_' || nc == '-' {
                        end = j + nc.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                if end > start {
                    result.push_str(&message[last_end..i]);
                    result.push('`');
                    result.push_str(&message[i..end]);
                    result.push('`');
                    last_end = end;
                    continue;
                }
            }
        }
    }
    result.push_str(&message[last_end..]);
    result
}

// ── Report dispatch ─────────────────────────────────────────────────────────

/// Whether the run is inside a GitHub Actions job.
///
/// Every subcommand's table output emits workflow annotations alongside
/// the table there, so a CI run annotates the diff without giving up the
/// readable summary (the behaviour PHPStan has).
pub(crate) fn in_github_actions() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some()
}

/// Render a report in `format` by calling the writer that belongs to it.
///
/// Only one of the three runs, except under [`OutputFormat::Table`]
/// inside GitHub Actions, where `github` runs first; see
/// [`in_github_actions`].
pub(crate) fn dispatch_report(
    format: OutputFormat,
    table: impl FnOnce(),
    github: impl FnOnce(),
    json: impl FnOnce(),
) {
    match format {
        OutputFormat::Table => {
            if in_github_actions() {
                github();
            }
            table();
        }
        OutputFormat::Github => github(),
        OutputFormat::Json => json(),
    }
}

// ── JSON output ─────────────────────────────────────────────────────────────

/// One entry of a report's `"files"` object: what was found in one file,
/// and how many findings that is.
///
/// `analyze` and `move` both report through this shape so a consumer can
/// read either with the same code.
#[derive(Serialize)]
pub(crate) struct JsonFileEntry<'a> {
    pub errors: usize,
    pub messages: Vec<JsonMessage<'a>>,
}

/// One finding inside a [`JsonFileEntry`].
#[derive(Serialize)]
pub(crate) struct JsonMessage<'a> {
    pub message: &'a str,
    pub line: u64,
    pub severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<&'a str>,
}

/// The `"totals"` object `analyze` and `move` share: findings that name
/// no file, and findings that do.
#[derive(Serialize)]
pub(crate) struct JsonTotals {
    pub errors: usize,
    pub file_errors: usize,
}

/// The whole `analyze` report.
///
/// `errors` is the top-level array for findings that name no file, which
/// `analyze` never produces; it is written as `[]` so the document keeps
/// the same shape `move` emits.
#[derive(Serialize)]
struct AnalyseReport<'a> {
    totals: JsonTotals,
    files: BTreeMap<&'a str, JsonFileEntry<'a>>,
    errors: [&'a str; 0],
}

/// Name a severity the way the JSON and GitHub formats spell it.
fn severity_name(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::ERROR => "error",
        DiagnosticSeverity::WARNING => "warning",
        DiagnosticSeverity::INFORMATION => "info",
        DiagnosticSeverity::HINT => "hint",
        _ => "unknown",
    }
}

/// Print all diagnostics as a single JSON object.
///
/// The format mirrors PHPStan's JSON output:
/// ```json
/// {
///   "totals": { "errors": 0, "file_errors": 42 },
///   "files": {
///     "src/Foo.php": {
///       "errors": 2,
///       "messages": [
///         { "message": "...", "line": 15, "severity": "error", "identifier": "unknown_class" }
///       ]
///     }
///   },
///   "errors": []
/// }
/// ```
pub(super) fn print_json_output(
    file_diagnostics: &[(String, Vec<FileDiagnostic>)],
    total_errors: usize,
) {
    println!("{}", analyse_json_body(file_diagnostics, total_errors));
}

/// Build the `analyze` JSON document; see [`print_json_output`].
fn analyse_json_body(
    file_diagnostics: &[(String, Vec<FileDiagnostic>)],
    total_errors: usize,
) -> String {
    let report = AnalyseReport {
        totals: JsonTotals {
            errors: 0,
            file_errors: total_errors,
        },
        files: file_diagnostics
            .iter()
            .map(|(path, diagnostics)| {
                (
                    path.as_str(),
                    JsonFileEntry {
                        errors: diagnostics.len(),
                        messages: diagnostics
                            .iter()
                            .map(|diag| JsonMessage {
                                message: &diag.message,
                                line: u64::from(diag.line),
                                severity: severity_name(diag.severity),
                                identifier: diag.identifier.as_deref(),
                            })
                            .collect(),
                    },
                )
            })
            .collect(),
        errors: [],
    };
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
}

// ── PHPStan-style table output ──────────────────────────────────────────────
//
// Mirrors Symfony Console's `Table` style used by PHPStan's
// `TableErrorFormatter` (see phpstan-src tests for exact spacing):
//
//  ------ -------------------------------------------
//   Line   src/Foo.php
//  ------ -------------------------------------------
//   15     Call to undefined method Bar::baz().
//          🪪  unknown_member
//   42     Access to property $qux on unknown class.
//          🪪  unknown_class
//  ------ -------------------------------------------

/// Print a file's diagnostics in the PHPStan table format.
pub(super) fn print_file_table(path: &str, diagnostics: &[FileDiagnostic], use_colour: bool) {
    let rows: Vec<TableRow> = diagnostics
        .iter()
        .map(|diag| TableRow {
            line: diag.line.to_string(),
            message: diag.message.clone(),
            detail: diag
                .identifier
                .as_ref()
                .map(|id| format!("\u{1faaa}  {id}")),
        })
        .collect();
    print_table(path, &rows, true, use_colour);
}

/// One row of a findings table: a line number, what was found, and an
/// optional dimmed second line naming the rule behind it.
pub(crate) struct TableRow {
    pub line: String,
    pub message: String,
    pub detail: Option<String>,
}

/// Print a PHPStan-style findings table for one file.
///
/// `measure_detail` decides whether the dimmed second lines widen the
/// message column. `analyze` sizes the table to fit them; `fix` sizes it
/// to the descriptions alone, because its rule names are long and
/// uniform enough that fitting them would push every table wide.
pub(crate) fn print_table(path: &str, rows: &[TableRow], measure_detail: bool, use_colour: bool) {
    let line_col_w = rows.iter().map(|r| r.line.len()).max().unwrap_or(0).max(4); // at least as wide as "Line"

    let msg_col_w = rows
        .iter()
        .flat_map(|r| {
            std::iter::once(r.message.len()).chain(
                r.detail
                    .as_ref()
                    .filter(|_| measure_detail)
                    .map(|d| d.len()),
            )
        })
        .max()
        .unwrap_or(0)
        .max(path.len());

    let sep = format!(
        " {} {}",
        "-".repeat(line_col_w + 2),
        "-".repeat(msg_col_w + 2),
    );

    // Header.
    println!("{sep}");
    if use_colour {
        println!(
            "  \x1b[32m{:>line_col_w$}\x1b[0m   \x1b[32m{path}\x1b[0m",
            "Line"
        );
    } else {
        println!("  {:>line_col_w$}   {path}", "Line");
    }
    println!("{sep}");

    // Data rows.
    for row in rows {
        println!("  {:>line_col_w$}   {}", row.line, row.message);
        if let Some(detail) = &row.detail {
            if use_colour {
                println!("  {:>line_col_w$}   \x1b[2m{detail}\x1b[0m", "");
            } else {
                println!("  {:>line_col_w$}   {detail}", "");
            }
        }
    }

    // Footer + blank line between files.
    println!("{sep}");
    println!();
}

/// The background a summary box is painted in.
#[derive(Clone, Copy)]
pub(crate) enum Colour {
    Green,
    Yellow,
    Red,
}

/// Print `text` in a summary box, the way every CLI subcommand closes.
///
/// Without colour the box is just the text, so piped output stays
/// greppable.
pub(crate) fn print_box(text: &str, colour: Colour, use_colour: bool) {
    if !use_colour {
        println!("{text}");
        return;
    }
    let sgr = match colour {
        Colour::Green => "30;42",
        Colour::Yellow => "30;43",
        Colour::Red => "97;41",
    };
    let pad = " ".repeat(text.len());
    println!();
    println!(" \x1b[{sgr}m{pad}\x1b[0m");
    println!(" \x1b[{sgr}m{text}\x1b[0m");
    println!(" \x1b[{sgr}m{pad}\x1b[0m");
    println!();
}

/// Print `text` in the green `[OK]` box a clean run ends with.
pub(crate) fn print_success_box(text: &str, use_colour: bool) {
    print_box(text, Colour::Green, use_colour);
}

/// Print the `[ERROR]` summary box.
pub(super) fn print_error_box(total_errors: usize, _file_count: usize, use_colour: bool) {
    let label = if total_errors == 1 { "error" } else { "errors" };
    let text = format!(" [ERROR] Found {total_errors} {label} ");
    print_box(&text, Colour::Red, use_colour);
}

// ── Progress bar ────────────────────────────────────────────────────────────

const BAR_WIDTH: usize = 28;

/// Render a PHPStan-style progress bar string, with an optional phase
/// label after it:
/// ` 120/883 [▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░]  13% Parsing`
pub(crate) fn progress_bar(done: usize, total: usize, label: &str) -> String {
    let pct = (done * 100).checked_div(total).unwrap_or(100);
    let filled = (done * BAR_WIDTH).checked_div(total).unwrap_or(BAR_WIDTH);
    let empty = BAR_WIDTH - filled;
    let separator = if label.is_empty() { "" } else { " " };

    format!(
        " {done:>width$}/{total} [{bar_fill}{bar_empty}] {pct:>3}%{separator}{label}",
        width = total.to_string().len(),
        bar_fill = "▓".repeat(filled),
        bar_empty = "░".repeat(empty),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic(line: u32, message: &str, identifier: Option<&str>) -> FileDiagnostic {
        FileDiagnostic {
            line,
            column: 0,
            message: message.to_string(),
            identifier: identifier.map(str::to_string),
            severity: DiagnosticSeverity::ERROR,
        }
    }

    #[test]
    fn json_is_valid_without_diagnostics() {
        let parsed: serde_json::Value =
            serde_json::from_str(&analyse_json_body(&[], 0)).expect("valid json");
        assert_eq!(parsed["totals"]["errors"], 0);
        assert_eq!(parsed["totals"]["file_errors"], 0);
        assert_eq!(parsed["files"], serde_json::json!({}));
        assert_eq!(parsed["errors"], serde_json::json!([]));
    }

    #[test]
    fn json_groups_diagnostics_by_file() {
        let files = vec![(
            "src/Foo.php".to_string(),
            vec![
                diagnostic(15, "a\"quoted\" \\ message\n", Some("unknown_member")),
                diagnostic(42, "plain", None),
            ],
        )];
        let parsed: serde_json::Value =
            serde_json::from_str(&analyse_json_body(&files, 2)).expect("valid json");
        assert_eq!(parsed["totals"]["file_errors"], 2);
        assert_eq!(parsed["files"]["src/Foo.php"]["errors"], 2);
        let messages = &parsed["files"]["src/Foo.php"]["messages"];
        assert_eq!(messages[0]["message"], "a\"quoted\" \\ message\n");
        assert_eq!(messages[0]["identifier"], "unknown_member");
        assert_eq!(messages[0]["severity"], "error");
        assert_eq!(messages[1]["line"], 42);
        // A diagnostic with no rule name leaves the key out entirely.
        assert!(messages[1].get("identifier").is_none());
    }

    #[test]
    fn json_escapes_control_characters() {
        let files = vec![(
            "a.php".to_string(),
            vec![diagnostic(1, "\u{0}\u{1f}", None)],
        )];
        let body = analyse_json_body(&files, 1);
        assert!(body.contains("\\u0000\\u001f"), "{body}");
    }

    #[test]
    fn github_annotation_leaves_out_an_empty_title() {
        assert_eq!(
            github_annotation("error", "src/Foo.php", 15, "", "boom"),
            "::error file=src/Foo.php,line=15,col=0::boom"
        );
        assert_eq!(
            github_annotation("warning", "src/Foo.php", 15, "rule", "boom"),
            "::warning file=src/Foo.php,line=15,col=0,title=rule::boom"
        );
    }
}
