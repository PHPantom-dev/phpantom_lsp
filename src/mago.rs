//! Mago proxy for AST-level and type-aware diagnostics.
//!
//! Mago is a Rust-based PHP toolchain that provides both a fast linter
//! (`mago lint`) and a type-aware analyser (`mago analyze`).  PHPantom
//! can proxy diagnostics from both commands.
//!
//! ## Auto-detection
//!
//! Mago is only activated when `mago.toml` exists at the workspace
//! root.  Even if the binary is available, PHPantom will not run Mago
//! without a configuration file.  The binary resolution chain is:
//!
//! 1. Explicit `.phpantom.toml` `command` value.
//! 2. `vendor/bin/mago` under the workspace root.
//! 3. `mago` on `$PATH`.
//!
//! Set `command = ""` to explicitly disable Mago.
//!
//! ## Configuration (`.phpantom.toml`)
//!
//! ```toml
//! [mago]
//! # Command/path for mago. When unset, auto-detected via
//! # vendor/bin/mago, then mago on $PATH.
//! # Set to "" to disable.
//! # command = "vendor/bin/mago"
//!
//! # Maximum runtime in milliseconds before `mago lint` is killed.
//! # Defaults to 30 000 ms (30 seconds).
//! # lint-timeout = 30000
//!
//! # Maximum runtime in milliseconds before `mago analyze` is killed.
//! # Defaults to 60 000 ms (60 seconds).
//! # analyze-timeout = 60000
//! ```
//!
//! ## Output parsing
//!
//! Both `mago lint` and `mago analyze` are invoked with
//! `--reporting-format json` and `--stdin-input`.  The buffer content
//! is piped to stdin and the real file path is passed as a positional
//! argument.  The JSON output contains an `issues` array with
//! structured annotations carrying byte offsets.  These are converted
//! to LSP `Diagnostic` values using the buffer content to compute
//! line/column positions.
//!
//! Requires Mago 1.15+ for `--stdin-input` support.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::config::MagoConfig;

// ── Tool resolution ─────────────────────────────────────────────────

/// A resolved Mago binary ready to invoke.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedMago {
    /// Absolute or relative path to the binary.
    pub path: PathBuf,
}

/// Check whether `mago.toml` exists at the workspace root.
///
/// Mago requires a configuration file to operate.  If `mago.toml` is
/// absent, we skip Mago entirely — even when the binary is available.
pub(crate) fn has_mago_config(workspace_root: &Path) -> bool {
    workspace_root.join("mago.toml").is_file()
}

/// Attempt to resolve the Mago binary from configuration and the
/// workspace environment.
///
/// Resolution rules:
/// - Config value `Some("")` (empty string) → disabled (`None`).
/// - Config value `Some(cmd)` → use `cmd` as-is (user override).
/// - Config value `None` → auto-detect: try `vendor/bin/mago` under
///   the workspace root, then search `$PATH`.
pub(crate) fn resolve_mago(
    workspace_root: Option<&Path>,
    config: &MagoConfig,
    bin_dir: Option<&str>,
) -> Option<ResolvedMago> {
    match config.command.as_deref() {
        // Explicitly disabled.
        Some("") => None,
        // User-provided command.
        Some(cmd) => Some(ResolvedMago {
            path: PathBuf::from(cmd),
        }),
        // Auto-detect.
        None => auto_detect(workspace_root, bin_dir),
    }
}

/// Auto-detect Mago by checking `<bin_dir>/mago` then `$PATH`.
fn auto_detect(workspace_root: Option<&Path>, bin_dir: Option<&str>) -> Option<ResolvedMago> {
    // Check the Composer bin directory first (vendor/bin/mago).
    if let Some(root) = workspace_root {
        let bin = bin_dir.unwrap_or("vendor/bin");
        let candidate = root.join(bin).join("mago");
        if candidate.is_file() {
            return Some(ResolvedMago { path: candidate });
        }
    }

    // Fall back to $PATH.
    if let Ok(path) = which("mago") {
        return Some(ResolvedMago { path });
    }

    None
}

/// Simple `which`-like lookup: search `$PATH` for an executable with
/// the given name.
fn which(binary_name: &str) -> Result<PathBuf, String> {
    let path_var = std::env::var("PATH").map_err(|_| "PATH not set".to_string())?;

    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() && is_executable(&candidate) {
            return Ok(candidate);
        }
    }

    Err(format!("{} not found on PATH", binary_name))
}

/// Check whether a file is executable.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

// ── Mago execution ─────────────────────────────────────────────────

/// Run `mago lint` on the given buffer content and return LSP diagnostics.
///
/// `file_path` is the real path of the file on disk.  `content` is the
/// current editor buffer (which may differ from the on-disk version).
///
/// Mago 1.15+ supports `--stdin-input`: pipe the buffer to stdin and
/// pass the real file path as a positional argument so that baseline
/// entries and issue locations use the correct path.  The editor buffer
/// content is written to stdin and stdin is closed before waiting.
///
/// `workspace_root` is needed to run Mago from the project root so that
/// it picks up `mago.toml`.
pub(crate) fn run_mago_lint(
    resolved: &ResolvedMago,
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    config: &MagoConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    let timeout_ms = config.lint_timeout_ms();
    let timeout = Duration::from_millis(timeout_ms);

    let mut cmd = Command::new(&resolved.path);
    cmd.arg("lint")
        .arg("--reporting-format")
        .arg("json")
        .arg("--stdin-input")
        .arg(file_path)
        .stdin(Stdio::piped())
        .current_dir(workspace_root);

    let file_path_str = file_path.to_string_lossy();
    let result = crate::util::run_command_with_timeout(
        &mut cmd,
        timeout,
        cancelled,
        "Mago lint",
        Some(content),
    )?;

    // Mago exit codes:
    //   0 = no issues found (may output "INFO No issues found." to stderr)
    //   1 = issues found
    //   2+ = error
    match result.code {
        0 => {
            // No issues — stdout may be empty or non-JSON.
            if result.stdout.trim().is_empty() {
                Ok(Vec::new())
            } else {
                match parse_mago_json(&result.stdout, content, &file_path_str, "mago-lint") {
                    Ok(diags) => Ok(diags),
                    Err(_) => Ok(Vec::new()),
                }
            }
        }
        1 => parse_mago_json(&result.stdout, content, &file_path_str, "mago-lint"),
        _ => {
            // For other exit codes, try parsing JSON; fall back
            // to error.
            match parse_mago_json(&result.stdout, content, &file_path_str, "mago-lint") {
                Ok(diags) if !diags.is_empty() => Ok(diags),
                _ => Err(format!(
                    "Mago lint exited with code {} (stderr: {})",
                    result.code,
                    result.stderr.trim()
                )),
            }
        }
    }
}

/// Run `mago analyze` on the given buffer content and return LSP diagnostics.
///
/// Same approach as [`run_mago_lint`] but invokes `mago analyze` which
/// performs slower, type-aware analysis.
pub(crate) fn run_mago_analyze(
    resolved: &ResolvedMago,
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    config: &MagoConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    let timeout_ms = config.analyze_timeout_ms();
    let timeout = Duration::from_millis(timeout_ms);

    let mut cmd = Command::new(&resolved.path);
    cmd.arg("analyze")
        .arg("--reporting-format")
        .arg("json")
        .arg("--stdin-input")
        .arg(file_path)
        .stdin(Stdio::piped())
        .current_dir(workspace_root);

    let file_path_str = file_path.to_string_lossy();
    let result = crate::util::run_command_with_timeout(
        &mut cmd,
        timeout,
        cancelled,
        "Mago analyze",
        Some(content),
    )?;

    match result.code {
        0 => {
            if result.stdout.trim().is_empty() {
                Ok(Vec::new())
            } else {
                match parse_mago_json(&result.stdout, content, &file_path_str, "mago-analyze") {
                    Ok(diags) => Ok(diags),
                    Err(_) => Ok(Vec::new()),
                }
            }
        }
        1 => parse_mago_json(&result.stdout, content, &file_path_str, "mago-analyze"),
        _ => match parse_mago_json(&result.stdout, content, &file_path_str, "mago-analyze") {
            Ok(diags) if !diags.is_empty() => Ok(diags),
            _ => Err(format!(
                "Mago analyze exited with code {} (stderr: {})",
                result.code,
                result.stderr.trim()
            )),
        },
    }
}

// ── JSON output parsing ─────────────────────────────────────────────

/// Parse Mago's JSON output into LSP diagnostics.
///
/// Both `mago lint` and `mago analyze` produce the same JSON format
/// when invoked with `--reporting-format json`:
///
/// ```json
/// {
///   "issues": [
///     {
///       "level": "Error",
///       "code": "invalid-return-statement",
///       "message": "Invalid return type...",
///       "notes": ["extra note text"],
///       "help": "helpful suggestion text",
///       "annotations": [
///         {
///           "message": "This has type...",
///           "kind": "Primary",
///           "span": {
///             "file_id": { "name": "...", "path": "..." },
///             "start": { "offset": 35, "line": 1 },
///             "end": { "offset": 42, "line": 1 }
///           }
///         }
///       ]
///     }
///   ]
/// }
/// ```
///
/// We filter annotations to only include those whose `span.file_id.path`
/// matches the file we ran against.  `content` is the original buffer
/// text, used to compute line/column positions from byte offsets.
fn parse_mago_json(
    json_str: &str,
    content: &str,
    file_path_str: &str,
    source_name: &str,
) -> Result<Vec<Diagnostic>, String> {
    let output: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse Mago JSON: {}", e))?;

    let mut diagnostics = Vec::new();

    if let Some(issues) = output.get("issues").and_then(|i| i.as_array()) {
        for issue in issues {
            if let Some(diag) = parse_mago_issue(issue, content, file_path_str, source_name) {
                diagnostics.push(diag);
            }
        }
    }

    Ok(diagnostics)
}

/// Parse a single Mago issue object into an LSP `Diagnostic`.
///
/// We look for the first `Primary` annotation whose file path matches
/// the temp file to determine the diagnostic range.  If no matching
/// primary annotation is found, the issue is skipped (it belongs to a
/// different file).
fn parse_mago_issue(
    issue: &serde_json::Value,
    content: &str,
    file_path_str: &str,
    source_name: &str,
) -> Option<Diagnostic> {
    let message = issue.get("message")?.as_str()?;
    let code = issue.get("code").and_then(|c| c.as_str()).unwrap_or("mago");

    let level = issue
        .get("level")
        .and_then(|l| l.as_str())
        .unwrap_or("Error");

    let severity = match level {
        "Error" => DiagnosticSeverity::ERROR,
        "Warning" => DiagnosticSeverity::WARNING,
        "Note" => DiagnosticSeverity::INFORMATION,
        "Help" => DiagnosticSeverity::HINT,
        _ => DiagnosticSeverity::ERROR,
    };

    // Find the primary annotation that matches our temp file.
    let annotations = issue.get("annotations").and_then(|a| a.as_array())?;

    let mut range: Option<Range> = None;
    let mut annotation_message: Option<&str> = None;

    for ann in annotations {
        let kind = ann.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind != "Primary" {
            continue;
        }

        // Check if this annotation is for our file.
        let span = ann.get("span")?;
        let file_path = span
            .get("file_id")
            .and_then(|f| f.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or("");

        if !paths_match(file_path, file_path_str) {
            continue;
        }

        let start_offset = span
            .get("start")
            .and_then(|s| s.get("offset"))
            .and_then(|o| o.as_u64())
            .unwrap_or(0) as usize;

        let end_offset = span
            .get("end")
            .and_then(|s| s.get("offset"))
            .and_then(|o| o.as_u64())
            .unwrap_or(start_offset as u64) as usize;

        let start_pos = byte_offset_to_position(content, start_offset);
        let end_pos = byte_offset_to_position(content, end_offset);

        range = Some(Range {
            start: start_pos,
            end: end_pos,
        });
        annotation_message = ann.get("message").and_then(|m| m.as_str());
        break;
    }

    // If no matching primary annotation, skip this issue.
    let diag_range = range?;

    // Build the full message: main message + annotation message + notes + help.
    let mut full_message = message.to_string();

    if let Some(ann_msg) = annotation_message.filter(|m| !m.is_empty() && *m != message) {
        full_message.push('\n');
        full_message.push_str(ann_msg);
    }

    if let Some(notes) = issue.get("notes").and_then(|n| n.as_array()) {
        for note in notes {
            if let Some(note_str) = note.as_str() {
                full_message.push_str("\nNote: ");
                full_message.push_str(note_str);
            }
        }
    }

    if let Some(help) = issue
        .get("help")
        .and_then(|h| h.as_str())
        .filter(|h| !h.is_empty())
    {
        full_message.push_str("\nHelp: ");
        full_message.push_str(help);
    }

    // Extract edits for the current file, if any.
    let data = issue
        .get("edits")
        .and_then(|e| e.as_array())
        .and_then(|edits_array| {
            let mut file_edits = Vec::new();
            for entry in edits_array {
                let tuple = entry.as_array()?;
                if tuple.len() != 2 {
                    continue;
                }
                let file_id = &tuple[0];
                let path = file_id.get("path").and_then(|p| p.as_str()).unwrap_or("");
                if !paths_match(path, file_path_str) {
                    continue;
                }
                let text_edits = tuple[1].as_array()?;
                for te in text_edits {
                    let range_obj = te.get("range")?;
                    let start = range_obj.get("start").and_then(|s| s.as_u64())?;
                    let end = range_obj.get("end").and_then(|e| e.as_u64())?;
                    let new_text = te.get("new_text").and_then(|t| t.as_str())?;
                    let safety = te.get("safety").and_then(|s| s.as_str()).unwrap_or("Safe");
                    file_edits.push(serde_json::json!({
                        "start": start,
                        "end": end,
                        "new_text": new_text,
                        "safety": safety,
                    }));
                }
            }
            if file_edits.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "mago_edits": file_edits }))
            }
        });

    Some(Diagnostic {
        range: diag_range,
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_string())),
        code_description: None,
        source: Some(source_name.to_string()),
        message: full_message,
        related_information: None,
        tags: None,
        data,
    })
}

/// Convert a byte offset within `content` to an LSP `Position`
/// (0-based line, UTF-16 character offset).
pub(crate) fn byte_offset_to_position(content: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in content.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    Position {
        line,
        character: col,
    }
}

/// Check whether two file paths refer to the same file.
///
/// Mago reports paths as absolute.  We compare by checking suffix
/// matches (one path ends with the other) to handle cases where one
/// path is relative and the other is absolute.
fn paths_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    // Normalize separators for comparison.
    let a_norm = a.replace('\\', "/");
    let b_norm = b.replace('\\', "/");
    if a_norm == b_norm {
        return true;
    }
    // Check suffix match (one is a suffix of the other), requiring a
    // path separator boundary so that e.g. "AFoo.php" does not match "Foo.php".
    a_norm.ends_with(&format!("/{}", b_norm)) || b_norm.ends_with(&format!("/{}", a_norm))
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── paths_match ─────────────────────────────────────────────────

    #[test]
    fn paths_match_identical() {
        assert!(paths_match(
            "/home/user/project/src/Foo.php",
            "/home/user/project/src/Foo.php"
        ));
    }

    #[test]
    fn paths_match_suffix() {
        assert!(paths_match("/home/user/project/src/Foo.php", "src/Foo.php"));
    }

    #[test]
    fn paths_match_different_files() {
        assert!(!paths_match(
            "/home/user/project/src/Foo.php",
            "src/Bar.php"
        ));
    }

    #[test]
    fn paths_match_rejects_partial_filename() {
        assert!(!paths_match("/project/src/AFoo.php", "Foo.php"));
    }

    // ── byte_offset_to_position ─────────────────────────────────────

    #[test]
    fn byte_offset_to_position_start_of_file() {
        let content = "<?php\necho 'hello';\n";
        let pos = byte_offset_to_position(content, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn byte_offset_to_position_second_line() {
        let content = "<?php\necho 'hello';\n";
        // Offset 6 is the 'e' of 'echo' on line 1.
        let pos = byte_offset_to_position(content, 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn byte_offset_to_position_mid_line() {
        let content = "<?php\necho 'hello';\n";
        // Offset 10 is the '\'' before 'hello' (line 1, col 4).
        let pos = byte_offset_to_position(content, 10);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 4);
    }

    #[test]
    fn byte_offset_to_position_end_of_content() {
        let content = "ab\ncd";
        // Offset 5 is past the last character.
        let pos = byte_offset_to_position(content, 5);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 2);
    }

    #[test]
    fn byte_offset_to_position_multibyte_char() {
        // '€' is 3 bytes in UTF-8 but 1 code unit in UTF-16.
        let content = "€x";
        let pos = byte_offset_to_position(content, 3); // byte offset of 'x'
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 1);
    }

    // ── parse_mago_json — lint issues ───────────────────────────────

    #[test]
    fn parse_lint_issues() {
        let content = "<?php\necho 'hello';\nreturn 42;\n";
        let file_path = "/tmp/phpantom-mago-abc123.php";

        let json = r#"{
            "issues": [
                {
                    "level": "Error",
                    "code": "invalid-return-statement",
                    "message": "Invalid return type.",
                    "notes": [],
                    "help": "",
                    "annotations": [
                        {
                            "message": "This has type int",
                            "kind": "Primary",
                            "span": {
                                "file_id": {
                                    "name": "test.php",
                                    "path": "/tmp/phpantom-mago-abc123.php",
                                    "size": 72,
                                    "file_type": "Host"
                                },
                                "start": { "offset": 20, "line": 2 },
                                "end": { "offset": 29, "line": 2 }
                            }
                        }
                    ]
                }
            ]
        }"#;

        let diags = parse_mago_json(json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diags[0].source.as_deref(), Some("mago-lint"));
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String(
                "invalid-return-statement".to_string()
            ))
        );
        assert!(diags[0].message.contains("Invalid return type."));
        assert!(diags[0].message.contains("This has type int"));
        // Line 2, offset 19 → compute from content
        assert_eq!(diags[0].range.start.line, 2);
    }

    // ── parse_mago_json — analyze issues ────────────────────────────

    #[test]
    fn parse_analyze_issues() {
        let content = "<?php\nfunction foo(): string { return 42; }\n";
        let file_path = "/tmp/phpantom-mago-xyz.php";

        let json = r#"{
            "issues": [
                {
                    "level": "Warning",
                    "code": "type-mismatch",
                    "message": "Type mismatch in return.",
                    "notes": ["expected string, got int"],
                    "help": "Change the return type or the value.",
                    "annotations": [
                        {
                            "message": "returns int here",
                            "kind": "Primary",
                            "span": {
                                "file_id": {
                                    "name": "test.php",
                                    "path": "/tmp/phpantom-mago-xyz.php",
                                    "size": 50,
                                    "file_type": "Host"
                                },
                                "start": { "offset": 35, "line": 1 },
                                "end": { "offset": 37, "line": 1 }
                            }
                        }
                    ]
                }
            ]
        }"#;

        let diags = parse_mago_json(json, content, file_path, "mago-analyze").unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(diags[0].source.as_deref(), Some("mago-analyze"));
        assert!(diags[0].message.contains("Type mismatch in return."));
        assert!(diags[0].message.contains("returns int here"));
        assert!(diags[0].message.contains("Note: expected string, got int"));
        assert!(
            diags[0]
                .message
                .contains("Help: Change the return type or the value.")
        );
    }

    // ── parse_mago_json — empty result ──────────────────────────────

    #[test]
    fn parse_empty_result() {
        let content = "<?php\n";
        let file_path = "/tmp/phpantom-mago-abc.php";
        let json = r#"{"issues": []}"#;
        let diags = parse_mago_json(json, content, file_path, "mago-lint").unwrap();
        assert!(diags.is_empty());
    }

    // ── severity mapping ────────────────────────────────────────────

    #[test]
    fn severity_mapping_error() {
        let content = "<?php\nfoo();\n";
        let file_path = "/tmp/test.php";
        let json = make_issue_json("Error", "err-code", "Error msg", "/tmp/test.php", 6, 11);
        let diags = parse_mago_json(&json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn severity_mapping_warning() {
        let content = "<?php\nfoo();\n";
        let file_path = "/tmp/test.php";
        let json = make_issue_json("Warning", "warn-code", "Warn msg", "/tmp/test.php", 6, 11);
        let diags = parse_mago_json(&json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn severity_mapping_note() {
        let content = "<?php\nfoo();\n";
        let file_path = "/tmp/test.php";
        let json = make_issue_json("Note", "note-code", "Note msg", "/tmp/test.php", 6, 11);
        let diags = parse_mago_json(&json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn severity_mapping_help() {
        let content = "<?php\nfoo();\n";
        let file_path = "/tmp/test.php";
        let json = make_issue_json("Help", "help-code", "Help msg", "/tmp/test.php", 6, 11);
        let diags = parse_mago_json(&json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::HINT));
    }

    // ── parse invalid JSON ──────────────────────────────────────────

    #[test]
    fn parse_invalid_json() {
        let result = parse_mago_json("not json", "", "Foo.php", "mago-lint");
        assert!(result.is_err());
    }

    // ── no matching file in annotations ─────────────────────────────

    #[test]
    fn parse_no_matching_file() {
        let content = "<?php\n";
        let file_path = "/tmp/phpantom-mago-abc.php";

        let json = r#"{
            "issues": [
                {
                    "level": "Error",
                    "code": "some-error",
                    "message": "Error in other file.",
                    "notes": [],
                    "help": "",
                    "annotations": [
                        {
                            "message": "here",
                            "kind": "Primary",
                            "span": {
                                "file_id": {
                                    "name": "other.php",
                                    "path": "/project/src/other.php",
                                    "size": 100,
                                    "file_type": "Host"
                                },
                                "start": { "offset": 0, "line": 0 },
                                "end": { "offset": 5, "line": 0 }
                            }
                        }
                    ]
                }
            ]
        }"#;

        let diags = parse_mago_json(json, content, file_path, "mago-lint").unwrap();
        assert!(diags.is_empty());
    }

    // ── has_mago_config ─────────────────────────────────────────────

    #[test]
    fn has_mago_config_true() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mago.toml"), "[linter]\n").unwrap();
        assert!(has_mago_config(dir.path()));
    }

    #[test]
    fn has_mago_config_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_mago_config(dir.path()));
    }

    // ── resolve_mago ────────────────────────────────────────────────

    #[test]
    fn resolve_disabled_when_empty_string() {
        let config = MagoConfig {
            command: Some(String::new()),
            lint_timeout: None,
            analyze_timeout: None,
        };
        let result = resolve_mago(None, &config, None);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_explicit_command() {
        let config = MagoConfig {
            command: Some("/usr/local/bin/mago".to_string()),
            lint_timeout: None,
            analyze_timeout: None,
        };
        let result = resolve_mago(None, &config, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, PathBuf::from("/usr/local/bin/mago"));
    }

    #[test]
    fn resolve_auto_detect_vendor_bin() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin_path).unwrap();
        let mago = bin_path.join("mago");
        std::fs::write(&mago, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mago, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = MagoConfig::default();
        let result = resolve_mago(Some(dir.path()), &config, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, mago);
    }

    #[test]
    fn resolve_auto_detect_custom_bin_dir() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("tools");
        std::fs::create_dir_all(&bin_path).unwrap();
        let mago = bin_path.join("mago");
        std::fs::write(&mago, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mago, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = MagoConfig::default();
        let result = resolve_mago(Some(dir.path()), &config, Some("tools"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, mago);
    }

    #[test]
    fn resolve_no_binary_found() {
        let dir = tempfile::tempdir().unwrap();
        let config = MagoConfig::default();
        // No vendor/bin/mago, and PATH is unlikely to have it in test env.
        // This test may still find mago on PATH in some environments,
        // so we just verify it doesn't panic.
        let _ = resolve_mago(Some(dir.path()), &config, None);
    }

    // ── timeout defaults ────────────────────────────────────────────

    #[test]
    fn lint_timeout_default() {
        let config = MagoConfig::default();
        assert_eq!(config.lint_timeout_ms(), 30_000);
    }

    #[test]
    fn lint_timeout_custom() {
        let config = MagoConfig {
            command: None,
            lint_timeout: Some(15_000),
            analyze_timeout: None,
        };
        assert_eq!(config.lint_timeout_ms(), 15_000);
    }

    #[test]
    fn analyze_timeout_default() {
        let config = MagoConfig::default();
        assert_eq!(config.analyze_timeout_ms(), 60_000);
    }

    #[test]
    fn analyze_timeout_custom() {
        let config = MagoConfig {
            command: None,
            lint_timeout: None,
            analyze_timeout: Some(120_000),
        };
        assert_eq!(config.analyze_timeout_ms(), 120_000);
    }

    // ── annotation message not duplicated when same as issue message ─

    #[test]
    fn annotation_message_not_duplicated_when_same() {
        let content = "<?php\nfoo();\n";
        let file_path = "/tmp/test.php";
        let json = r#"{
            "issues": [
                {
                    "level": "Error",
                    "code": "test",
                    "message": "Same message",
                    "notes": [],
                    "help": "",
                    "annotations": [
                        {
                            "message": "Same message",
                            "kind": "Primary",
                            "span": {
                                "file_id": {
                                    "name": "test.php",
                                    "path": "/tmp/test.php",
                                    "size": 14,
                                    "file_type": "Host"
                                },
                                "start": { "offset": 6, "line": 1 },
                                "end": { "offset": 11, "line": 1 }
                            }
                        }
                    ]
                }
            ]
        }"#;
        let diags = parse_mago_json(json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags.len(), 1);
        // Message should NOT be duplicated.
        assert_eq!(diags[0].message, "Same message");
    }

    // ── Helper to build issue JSON for severity tests ───────────────

    #[test]
    fn parse_edits_from_json() {
        let content = "<?php\nfoo();\n";
        let file_path = "/tmp/test.php";
        let json = r#"{
            "issues": [
                {
                    "level": "Warning",
                    "code": "fixable",
                    "message": "Use bar() instead",
                    "notes": [],
                    "help": "",
                    "annotations": [
                        {
                            "message": "",
                            "kind": "Primary",
                            "span": {
                                "file_id": {
                                    "name": "test.php",
                                    "path": "/tmp/test.php",
                                    "size": 14,
                                    "file_type": "Host"
                                },
                                "start": { "offset": 6, "line": 1 },
                                "end": { "offset": 11, "line": 1 }
                            }
                        }
                    ],
                    "edits": [
                        [
                            { "name": "test.php", "path": "/tmp/test.php", "size": 14, "file_type": "Host" },
                            [
                                { "range": { "start": 6, "end": 11 }, "new_text": "bar()", "safety": "Safe" }
                            ]
                        ],
                        [
                            { "name": "other.php", "path": "/tmp/other.php", "size": 20, "file_type": "Host" },
                            [
                                { "range": { "start": 0, "end": 5 }, "new_text": "baz()", "safety": "Unsafe" }
                            ]
                        ]
                    ]
                }
            ]
        }"#;
        let diags = parse_mago_json(json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags.len(), 1);

        let data = diags[0].data.as_ref().expect("diagnostic should have data");
        let mago_edits = data
            .get("mago_edits")
            .expect("should have mago_edits key")
            .as_array()
            .expect("mago_edits should be an array");
        assert_eq!(mago_edits.len(), 1);
        assert_eq!(mago_edits[0]["start"], 6);
        assert_eq!(mago_edits[0]["end"], 11);
        assert_eq!(mago_edits[0]["new_text"], "bar()");
        assert_eq!(mago_edits[0]["safety"], "Safe");
    }

    #[test]
    fn parse_no_edits_leaves_data_none() {
        let content = "<?php\nfoo();\n";
        let file_path = "/tmp/test.php";
        let json = make_issue_json("Error", "test", "some error", file_path, 6, 11);
        let diags = parse_mago_json(&json, content, file_path, "mago-lint").unwrap();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].data.is_none());
    }

    fn make_issue_json(
        level: &str,
        code: &str,
        message: &str,
        path: &str,
        start_offset: u64,
        end_offset: u64,
    ) -> String {
        format!(
            r#"{{
                "issues": [
                    {{
                        "level": "{}",
                        "code": "{}",
                        "message": "{}",
                        "notes": [],
                        "help": "",
                        "annotations": [
                            {{
                                "message": "",
                                "kind": "Primary",
                                "span": {{
                                    "file_id": {{
                                        "name": "test.php",
                                        "path": "{}",
                                        "size": 100,
                                        "file_type": "Host"
                                    }},
                                    "start": {{ "offset": {}, "line": 1 }},
                                    "end": {{ "offset": {}, "line": 1 }}
                                }}
                            }}
                        ]
                    }}
                ]
            }}"#,
            level, code, message, path, start_offset, end_offset
        )
    }
}
