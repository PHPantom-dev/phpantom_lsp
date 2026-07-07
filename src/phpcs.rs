//! PHPCS (PHP_CodeSniffer) proxy for coding standard diagnostics.
//!
//! PHPantom can proxy diagnostics from PHP_CodeSniffer by running
//! `phpcs --report=json` on the current file.  This surfaces coding
//! standard violations (PSR-12, PSR-1, custom sniffs) as LSP
//! diagnostics.
//!
//! ## Auto-detection
//!
//! When `command` is unset, PHPantom checks whether
//! `squizlabs/php_codesniffer` is in `require-dev` and resolves the
//! `phpcs` binary via Composer's bin-dir, then falls back to `$PATH`.
//! Set `command = ""` to explicitly disable PHPCS.
//!
//! ## Configuration (`.phpantom.toml`)
//!
//! ```toml
//! [phpcs]
//! # Command/path for phpcs. When unset, auto-detected via
//! # Composer's bin-dir (from require-dev), then $PATH.
//! # Set to "" to disable.
//! # command = "vendor/bin/phpcs"
//!
//! # Coding standard. When unset, PHPCS uses its own default
//! # detection (phpcs.xml / phpcs.xml.dist in project root,
//! # then its built-in default).
//! # standard = "PSR12"
//!
//! # Maximum runtime in milliseconds before PHPCS is killed.
//! # Defaults to 30 000 ms (30 seconds).
//! # timeout = 30000
//! ```
//!
//! ## Output parsing
//!
//! PHPCS is invoked with `--report=json` and the JSON output is parsed
//! to extract file-level messages which are converted to LSP
//! `Diagnostic` values.  Each violation maps to a diagnostic with the
//! sniff name as the code (e.g. `PSR12.Files.FileHeader.MissingPHPVersion`).
//! Fixable violations are marked in `Diagnostic.data` so that a companion
//! code action can offer `phpcbf` auto-fix.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::config::PhpcsConfig;

/// Default PHPCS timeout in milliseconds (30 seconds).
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

// ── Tool resolution ─────────────────────────────────────────────────

/// A resolved PHPCS binary ready to invoke.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedPhpcs {
    /// Absolute or relative path to the binary.
    pub path: PathBuf,
}

/// Attempt to resolve the PHPCS binary from configuration and the
/// workspace environment.
///
/// Resolution rules:
/// - Config value `Some("")` (empty string) → disabled (`None`).
/// - Config value `Some(cmd)` → use `cmd` as-is (user override).
/// - Config value `None` → auto-detect: try `<bin_dir>/phpcs` under
///   the workspace root, then search `$PATH`.
pub(crate) fn resolve_phpcs(
    workspace_root: Option<&Path>,
    config: &PhpcsConfig,
    bin_dir: Option<&str>,
) -> Option<ResolvedPhpcs> {
    match config.command.as_deref() {
        // Explicitly disabled.
        Some("") => None,
        // User-provided command.
        Some(cmd) => Some(ResolvedPhpcs {
            path: PathBuf::from(cmd),
        }),
        // Auto-detect.
        None => auto_detect(workspace_root, bin_dir),
    }
}

/// Auto-detect PHPCS by checking `<bin_dir>/phpcs` then `$PATH`.
fn auto_detect(workspace_root: Option<&Path>, bin_dir: Option<&str>) -> Option<ResolvedPhpcs> {
    // Check the Composer bin directory first.
    if let Some(root) = workspace_root {
        let bin = bin_dir.unwrap_or("vendor/bin");
        let candidate = root.join(bin).join("phpcs");
        if candidate.is_file() {
            return Some(ResolvedPhpcs { path: candidate });
        }
    }

    // Fall back to $PATH.
    if let Ok(path) = which("phpcs") {
        return Some(ResolvedPhpcs { path });
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

// ── PHPCS execution ─────────────────────────────────────────────────

/// Run PHPCS on the given buffer content and return LSP diagnostics.
///
/// `file_path` is the real path of the file on disk.  `content` is the
/// current editor buffer (which may differ from the on-disk version).
/// PHPCS reads from stdin when the `-` argument is given, and
/// `--stdin-path` tells it the original filename for ruleset matching.
///
/// `workspace_root` is needed to run PHPCS from the project root
/// directory so that it picks up `phpcs.xml` / `phpcs.xml.dist`.
pub(crate) fn run_phpcs(
    resolved: &ResolvedPhpcs,
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    config: &PhpcsConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    let timeout_ms = config.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let timeout = Duration::from_millis(timeout_ms);

    let mut cmd = Command::new(&resolved.path);
    cmd.arg("--report=json")
        .arg("--no-colors")
        .arg("-q")
        .arg(format!("--stdin-path={}", file_path.display()))
        .current_dir(workspace_root);

    if let Some(ref standard) = config.standard {
        cmd.arg(format!("--standard={}", standard));
    }

    // The `-` argument tells PHPCS to read from stdin.
    cmd.arg("-");

    // The buffer content is fed to PHPCS's stdin by the shared helper,
    // which drains stdout/stderr concurrently so a large JSON report
    // cannot deadlock against a full pipe buffer.
    let result =
        crate::util::run_command_with_timeout(&mut cmd, timeout, cancelled, "PHPCS", Some(content));

    match result {
        Ok(output) => {
            // PHPCS exit codes:
            //   0 = no violations found
            //   1 = violations found (warnings only)
            //   2 = violations found (errors present)
            //   3 = processing error
            match output.code {
                0 => Ok(Vec::new()),
                1 | 2 => parse_phpcs_json(&output.stdout, file_path),
                _ => {
                    // For other exit codes, try parsing JSON; fall back
                    // to error.
                    match parse_phpcs_json(&output.stdout, file_path) {
                        Ok(diags) if !diags.is_empty() => Ok(diags),
                        _ => Err(format!(
                            "PHPCS exited with code {} (stderr: {})",
                            output.code,
                            output.stderr.trim()
                        )),
                    }
                }
            }
        }
        Err(e) => Err(e),
    }
}

// ── JSON output parsing ─────────────────────────────────────────────

/// Parse PHPCS's JSON output into LSP diagnostics.
///
/// PHPCS JSON format (with `--report=json`):
///
/// ```json
/// {
///   "totals": {
///     "errors": 1,
///     "warnings": 1,
///     "fixable": 2
///   },
///   "files": {
///     "/path/to/file.php": {
///       "errors": 1,
///       "warnings": 1,
///       "messages": [
///         {
///           "message": "Line indented incorrectly; expected 4 spaces, found 2",
///           "source": "PSR2.Methods.FunctionCallSignature.Indent",
///           "severity": 5,
///           "fixable": true,
///           "type": "ERROR",
///           "line": 42,
///           "column": 1
///         }
///       ]
///     }
///   }
/// }
/// ```
///
/// We extract messages for the file being edited (matching by path).
/// When using stdin mode with `--stdin-path`, PHPCS keys the output
/// by the `--stdin-path` value.  When there is only one file entry,
/// we use it regardless of the key to avoid path-matching issues.
fn parse_phpcs_json(json_str: &str, file_path: &Path) -> Result<Vec<Diagnostic>, String> {
    let output: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse PHPCS JSON: {}", e))?;

    let mut diagnostics = Vec::new();

    if let Some(files) = output.get("files").and_then(|f| f.as_object()) {
        // When using stdin mode with --stdin-path, PHPCS keys the output
        // by the stdin-path value.  Try matching by the path, and if
        // there's only one file entry, use it regardless of key.
        let messages = if files.len() == 1 {
            files.values().next()
        } else {
            let file_path_str = file_path.to_string_lossy();
            files
                .iter()
                .find(|(path, _)| paths_match(path, &file_path_str))
                .map(|(_, v)| v)
        };

        if let Some(msgs) = messages
            .and_then(|fd| fd.get("messages"))
            .and_then(|m| m.as_array())
        {
            for msg in msgs {
                if let Some(diag) = parse_phpcs_message(msg) {
                    diagnostics.push(diag);
                }
            }
        }
    }

    Ok(diagnostics)
}

/// Parse a single PHPCS message object into an LSP `Diagnostic`.
fn parse_phpcs_message(msg: &serde_json::Value) -> Option<Diagnostic> {
    let message = msg.get("message")?.as_str()?;
    let line = msg.get("line").and_then(|l| l.as_u64()).unwrap_or(1);
    let lsp_line = line.saturating_sub(1) as u32;

    // PHPCS "source" is the sniff name, e.g. "PSR2.Methods.FunctionCallSignature.Indent"
    let source_code = msg
        .get("source")
        .and_then(|s| s.as_str())
        .unwrap_or("phpcs");

    // PHPCS "type" is "ERROR" or "WARNING"
    let severity = match msg.get("type").and_then(|t| t.as_str()) {
        Some("ERROR") => DiagnosticSeverity::ERROR,
        _ => DiagnosticSeverity::WARNING,
    };

    let fixable = msg
        .get("fixable")
        .and_then(|f| f.as_bool())
        .unwrap_or(false);

    let data = Some(serde_json::json!({ "fixable": fixable }));

    // PHPCS reports a single `column` per message, but its meaning
    // varies by sniff: for `LineLength.TooLong` it is the total line
    // length (the *end*), for indentation sniffs it is the offending
    // token start, etc.  Because there is no reliable way to derive a
    // precise range from a single ambiguous position, we underline the
    // full line — the same strategy PHPStan uses.
    Some(Diagnostic {
        range: Range {
            start: Position {
                line: lsp_line,
                character: 0,
            },
            end: Position {
                line: lsp_line,
                character: u32::MAX,
            },
        },
        severity: Some(severity),
        code: Some(NumberOrString::String(source_code.to_string())),
        code_description: None,
        source: Some("phpcs".to_string()),
        message: message.to_string(),
        related_information: None,
        tags: None,
        data,
    })
}

/// Check whether two file paths refer to the same file.
///
/// PHPCS may use the `--stdin-path` value as the key. We compare by
/// checking suffix matches (one path ends with the other) to handle
/// cases where one path is relative and the other is absolute, or
/// where symlinks produce different prefixes.
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

    // ── parse_phpcs_json ────────────────────────────────────────────

    #[test]
    fn parse_empty_result() {
        let json = r#"{"totals":{"errors":0,"warnings":0,"fixable":0},"files":{}}"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn parse_file_messages() {
        let json = r#"{
            "totals": {"errors": 1, "warnings": 1, "fixable": 2},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "warnings": 1,
                    "messages": [
                        {
                            "message": "Line indented incorrectly; expected 4 spaces, found 2",
                            "source": "PSR2.Methods.FunctionCallSignature.Indent",
                            "severity": 5,
                            "fixable": true,
                            "type": "ERROR",
                            "line": 42,
                            "column": 1
                        },
                        {
                            "message": "Missing file doc comment",
                            "source": "PEAR.Commenting.FileComment.Missing",
                            "severity": 5,
                            "fixable": false,
                            "type": "WARNING",
                            "line": 1,
                            "column": 1
                        }
                    ]
                }
            }
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert_eq!(diags.len(), 2);

        // First diagnostic — ERROR, full-line range
        assert_eq!(diags[0].range.start.line, 41); // 42 - 1
        assert_eq!(diags[0].range.start.character, 0);
        assert_eq!(diags[0].range.end.line, 41);
        assert_eq!(diags[0].range.end.character, u32::MAX);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diags[0].source.as_deref(), Some("phpcs"));
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String(
                "PSR2.Methods.FunctionCallSignature.Indent".to_string()
            ))
        );
        assert!(diags[0].message.contains("Line indented incorrectly"));
        assert_eq!(diags[0].data, Some(serde_json::json!({ "fixable": true })));

        // Second diagnostic — WARNING
        assert_eq!(diags[1].range.start.line, 0); // 1 - 1
        assert_eq!(diags[1].range.end.character, u32::MAX);
        assert_eq!(diags[1].severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            diags[1].code,
            Some(NumberOrString::String(
                "PEAR.Commenting.FileComment.Missing".to_string()
            ))
        );
        assert_eq!(diags[1].data, Some(serde_json::json!({ "fixable": false })));
    }

    #[test]
    fn parse_fixable_flag() {
        let json_fixable = r#"{
            "totals": {"errors": 1, "warnings": 0, "fixable": 1},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "warnings": 0,
                    "messages": [
                        {
                            "message": "Expected 1 space after comma.",
                            "source": "Generic.Functions.FunctionCallArgumentSpacing.NoSpaceAfterComma",
                            "severity": 5,
                            "fixable": true,
                            "type": "ERROR",
                            "line": 10,
                            "column": 15
                        }
                    ]
                }
            }
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json_fixable, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].data, Some(serde_json::json!({ "fixable": true })));

        let json_not_fixable = r#"{
            "totals": {"errors": 1, "warnings": 0, "fixable": 0},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "warnings": 0,
                    "messages": [
                        {
                            "message": "Class name must be declared in StudlyCaps.",
                            "source": "PSR1.Classes.ClassDeclaration.MissingNamespace",
                            "severity": 5,
                            "fixable": false,
                            "type": "ERROR",
                            "line": 3,
                            "column": 7
                        }
                    ]
                }
            }
        }"#;
        let diags = parse_phpcs_json(json_not_fixable, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].data, Some(serde_json::json!({ "fixable": false })));
    }

    #[test]
    fn parse_single_file_entry_always_matches() {
        let json = r#"{
            "totals": {"errors": 1, "warnings": 0, "fixable": 0},
            "files": {
                "STDIN": {
                    "errors": 1,
                    "warnings": 0,
                    "messages": [
                        {
                            "message": "Some violation.",
                            "source": "Generic.Sniff.Name",
                            "severity": 5,
                            "fixable": false,
                            "type": "ERROR",
                            "line": 5,
                            "column": 1
                        }
                    ]
                }
            }
        }"#;
        // The key is "STDIN" which does not match the file path at all,
        // but since there is only one file entry, we use it.
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].range.start.line, 4); // 5 - 1
    }

    #[test]
    fn parse_no_matching_file() {
        let json = r#"{
            "totals": {"errors": 1, "warnings": 0, "fixable": 0},
            "files": {
                "/project/src/Bar.php": {
                    "errors": 1,
                    "warnings": 0,
                    "messages": [
                        {
                            "message": "Error in Bar.",
                            "source": "Generic.Sniff.Name",
                            "severity": 5,
                            "fixable": false,
                            "type": "ERROR",
                            "line": 1,
                            "column": 1
                        }
                    ]
                },
                "/project/src/Baz.php": {
                    "errors": 0,
                    "warnings": 0,
                    "messages": []
                }
            }
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn parse_invalid_json() {
        let result = parse_phpcs_json("not json", Path::new("Foo.php"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_message_line_zero_defaults_to_line_1() {
        let json = r#"{
            "totals": {"errors": 1, "warnings": 0, "fixable": 0},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "warnings": 0,
                    "messages": [
                        {
                            "message": "Error at line zero.",
                            "source": "Generic.Sniff.Name",
                            "severity": 5,
                            "fixable": false,
                            "type": "ERROR",
                            "line": 0,
                            "column": 1
                        }
                    ]
                }
            }
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        // Line 0 saturating_sub(1) = 0
        assert_eq!(diags[0].range.start.line, 0);
    }

    // ── resolve_phpcs ───────────────────────────────────────────────

    #[test]
    fn resolve_disabled_when_empty_string() {
        let config = PhpcsConfig {
            command: Some(String::new()),
            standard: None,
            timeout: None,
        };
        let result = resolve_phpcs(None, &config, None);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_explicit_command() {
        let config = PhpcsConfig {
            command: Some("custom/phpcs".to_string()),
            standard: None,
            timeout: None,
        };
        let result = resolve_phpcs(None, &config, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, PathBuf::from("custom/phpcs"));
    }

    // ── PhpcsConfig helpers ─────────────────────────────────────────

    #[test]
    fn config_timeout_default() {
        let config = PhpcsConfig::default();
        assert_eq!(config.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn config_timeout_custom() {
        let config = PhpcsConfig {
            command: None,
            standard: None,
            timeout: Some(15_000),
        };
        assert_eq!(config.timeout_ms(), 15_000);
    }

    #[test]
    fn config_is_disabled() {
        let disabled = PhpcsConfig {
            command: Some(String::new()),
            standard: None,
            timeout: None,
        };
        assert!(disabled.is_disabled());

        let enabled = PhpcsConfig::default();
        assert!(!enabled.is_disabled());

        let explicit = PhpcsConfig {
            command: Some("vendor/bin/phpcs".to_string()),
            standard: None,
            timeout: None,
        };
        assert!(!explicit.is_disabled());
    }

    #[test]
    fn parse_warning_severity() {
        let json = r#"{
            "totals": {"errors": 0, "warnings": 1, "fixable": 0},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 0,
                    "warnings": 1,
                    "messages": [
                        {
                            "message": "Line exceeds 120 characters.",
                            "source": "Generic.Files.LineLength.TooLong",
                            "severity": 5,
                            "fixable": false,
                            "type": "WARNING",
                            "line": 50,
                            "column": 121
                        }
                    ]
                }
            }
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn parse_full_line_range() {
        let json = r#"{
            "totals": {"errors": 1, "warnings": 0, "fixable": 0},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "warnings": 0,
                    "messages": [
                        {
                            "message": "Whitespace found.",
                            "source": "Squiz.WhiteSpace.SuperfluousWhitespace.EndLine",
                            "severity": 5,
                            "fixable": true,
                            "type": "ERROR",
                            "line": 10,
                            "column": 5
                        }
                    ]
                }
            }
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        // Full-line range: column is ignored because its semantics
        // vary by sniff (token start vs line length vs other).
        assert_eq!(diags[0].range.start.line, 9);
        assert_eq!(diags[0].range.start.character, 0);
        assert_eq!(diags[0].range.end.line, 9);
        assert_eq!(diags[0].range.end.character, u32::MAX);
    }

    #[test]
    fn parse_stdin_path_key() {
        // When PHPCS is invoked with --stdin-path=/project/src/Foo.php,
        // it reports the file under that path value.
        let json = r#"{
            "totals": {"errors": 1, "warnings": 0, "fixable": 0},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "warnings": 0,
                    "messages": [
                        {
                            "message": "Missing namespace declaration.",
                            "source": "PSR1.Classes.ClassDeclaration.MissingNamespace",
                            "severity": 5,
                            "fixable": false,
                            "type": "ERROR",
                            "line": 2,
                            "column": 1
                        }
                    ]
                },
                "/project/src/Bar.php": {
                    "errors": 0,
                    "warnings": 0,
                    "messages": []
                }
            }
        }"#;
        // With multiple file entries, path matching is used.
        // The --stdin-path value matches the requested file.
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpcs_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Missing namespace"));
    }
}
