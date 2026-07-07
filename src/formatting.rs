//! Built-in formatting with external tool override.
//!
//! PHPantom ships a built-in PHP formatter (mago-formatter) that works
//! out of the box.  Projects that depend on Laravel Pint, php-cs-fixer,
//! or PHP_CodeSniffer in their `composer.json` `require-dev`
//! automatically use those tools instead.  Users can also override tool
//! paths or disable formatting entirely via `.phpantom.toml`.
//!
//! ## Resolution strategy
//!
//! 1. **Explicit config wins.**  If the user sets a tool path in
//!    `.phpantom.toml`, use that tool.  If they set it to `""`, that
//!    tool is disabled.
//! 2. **Composer `require-dev` wins over built-in.**  If
//!    `composer.json` lists `laravel/pint`,
//!    `friendsofphp/php-cs-fixer`, or `squizlabs/php_codesniffer` in
//!    `require-dev`, resolve the binary via Composer's bin-dir and run
//!    it as a subprocess.
//! 3. **Otherwise, use mago-formatter.**  No subprocess, no temp files,
//!    no external dependencies.  Uses PER-CS 2.0 defaults.
//!
//! ## Configuration (`.phpantom.toml`)
//!
//! ```toml
//! [formatting]
//! # Explicit path: always use this tool, skip require-dev detection.
//! # pint = "/usr/local/bin/pint"
//! # php-cs-fixer = "/usr/local/bin/php-cs-fixer"
//!
//! # Empty string: disable this tool entirely.
//! # pint = ""
//! # php-cs-fixer = ""
//!
//! # Omitted (default): check require-dev, then fall back to
//! # mago-formatter.
//!
//! # Timeout applies to external tools only.
//! # timeout = 10000
//! ```
//!
//! ## Config file discovery
//!
//! External tools discover their project config by walking up from
//! the file being formatted.  File-based tools (php-cs-fixer, phpcbf)
//! run on a sibling temp file in the same directory as the original so
//! that config walkers (`.php-cs-fixer.php`, `.phpcs.xml`, etc.) find
//! the project rules.  Pint uses `--stdin-filename` to achieve the
//! same config discovery without temp files.

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use tempfile::NamedTempFile;

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::atom::bytes_to_str;
use crate::config::FormattingConfig;

/// Default formatting timeout in milliseconds.
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

// ── Tool resolution ─────────────────────────────────────────────────

/// A resolved formatting tool ready to invoke.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedTool {
    /// Human-readable name for logging.
    pub name: &'static str,
    /// Absolute or relative path to the binary.
    pub path: PathBuf,
}

/// The resolved formatting strategy: external tools, built-in
/// formatter, or disabled.
#[derive(Debug)]
pub(crate) enum FormattingStrategy {
    /// Run one or more external tools in sequence.
    External(Vec<ResolvedTool>),
    /// Use the built-in mago-formatter.
    BuiltIn,
    /// Formatting is explicitly disabled.
    Disabled,
}

/// Resolve the formatting strategy from config, Composer metadata, and
/// the workspace root.
///
/// Resolution rules:
/// - If `config.is_disabled()` (both tools set to `""`) → `Disabled`.
/// - If either tool has an explicit non-empty path in config →
///   `External` with those tools.
/// - If `composer_json` has `friendsofphp/php-cs-fixer` or
///   `squizlabs/php_codesniffer` in `require-dev` → `External`,
///   resolving paths via the Composer bin-dir.
/// - Otherwise → `BuiltIn`.
pub(crate) fn resolve_strategy(
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    composer_json: Option<&crate::composer::ComposerPackage>,
    bin_dir: Option<&str>,
) -> FormattingStrategy {
    if config.is_disabled() {
        return FormattingStrategy::Disabled;
    }

    // Check for explicit config overrides first.
    let fixer_explicit = matches!(config.php_cs_fixer.as_deref(), Some(s) if !s.is_empty());
    let phpcbf_explicit = matches!(config.phpcbf.as_deref(), Some(s) if !s.is_empty());
    let pint_explicit = matches!(config.pint.as_deref(), Some(s) if !s.is_empty());

    if fixer_explicit || phpcbf_explicit || pint_explicit {
        let mut tools = Vec::new();
        if let Some(cmd) = config.pint.as_deref()
            && !cmd.is_empty()
        {
            tools.push(ResolvedTool {
                name: "pint",
                path: PathBuf::from(cmd),
            });
        }
        if let Some(cmd) = config.php_cs_fixer.as_deref()
            && !cmd.is_empty()
        {
            tools.push(ResolvedTool {
                name: "php-cs-fixer",
                path: PathBuf::from(cmd),
            });
        }
        if let Some(cmd) = config.phpcbf.as_deref()
            && !cmd.is_empty()
        {
            tools.push(ResolvedTool {
                name: "phpcbf",
                path: PathBuf::from(cmd),
            });
        }
        if tools.is_empty() {
            return FormattingStrategy::Disabled;
        }
        return FormattingStrategy::External(tools);
    }

    // No explicit config — check composer.json require-dev.
    if let Some(package) = composer_json {
        let mut tools = Vec::new();
        let bin = bin_dir.unwrap_or("vendor/bin");

        // Only one of the config values can be Some("") here (disabling
        // one tool while leaving the other to auto-detect).
        let fixer_disabled = config.php_cs_fixer.as_deref() == Some("");
        let phpcbf_disabled = config.phpcbf.as_deref() == Some("");
        let pint_disabled = config.pint.as_deref() == Some("");

        if !pint_disabled
            && crate::composer::has_require_dev(package, "laravel/pint")
            && let Some(tool) = resolve_from_bin_dir("pint", workspace_root, bin)
        {
            tools.push(tool);
        }

        if !fixer_disabled
            && crate::composer::has_require_dev(package, "friendsofphp/php-cs-fixer")
            && let Some(tool) = resolve_from_bin_dir("php-cs-fixer", workspace_root, bin)
        {
            tools.push(tool);
        }

        if !phpcbf_disabled
            && crate::composer::has_require_dev(package, "squizlabs/php_codesniffer")
            && let Some(tool) = resolve_from_bin_dir("phpcbf", workspace_root, bin)
        {
            tools.push(tool);
        }

        if !tools.is_empty() {
            return FormattingStrategy::External(tools);
        }
    }

    // No external tools configured or detected — use built-in.
    FormattingStrategy::BuiltIn
}

/// Resolve a tool binary from the Composer bin directory.
fn resolve_from_bin_dir(
    binary_name: &'static str,
    workspace_root: Option<&Path>,
    bin_dir: &str,
) -> Option<ResolvedTool> {
    let root = workspace_root?;
    let candidate = root.join(bin_dir).join(binary_name);
    if candidate.is_file() {
        Some(ResolvedTool {
            name: binary_name,
            path: candidate,
        })
    } else {
        None
    }
}

// ── Built-in formatter ──────────────────────────────────────────────

/// Format PHP source code using the built-in mago-formatter.
///
/// Returns the formatted source string, or an error if parsing fails.
/// Uses PER-CS 2.0 style defaults.
fn format_with_mago(
    content: &str,
    php_version: mago_php_version::PHPVersion,
) -> Result<String, String> {
    let arena = bumpalo::Bump::new();
    let settings = mago_formatter::settings::FormatSettings::default();
    let formatter = mago_formatter::Formatter::new(&arena, php_version, settings);

    let formatted = formatter
        .format_code(
            Cow::Borrowed(b"phpantom-fmt"),
            Cow::Owned(content.as_bytes().to_vec()),
        )
        .map_err(|e| format!("Built-in formatter failed to parse PHP: {}", e))?;

    Ok(bytes_to_str(formatted).to_string())
}

/// Convert a project [`PhpVersion`](crate::types::PhpVersion) into the
/// mago-formatter's [`PHPVersion`](mago_php_version::PHPVersion).
fn to_mago_php_version(v: crate::types::PhpVersion) -> mago_php_version::PHPVersion {
    mago_php_version::PHPVersion::new(v.major as u32, v.minor as u32, 0)
}

// ── External tool pipeline ──────────────────────────────────────────

/// Run the external tool pipeline on `content` and return `TextEdit`s.
///
/// Each tool in the pipeline runs in sequence.  The output of one tool
/// becomes the input for the next.  The final result is diffed against
/// the original content to produce edits.
///
/// `file_path` is the real path of the file on disk, used so that
/// sibling temp files land in the correct directory for tool config
/// discovery.
fn run_external_pipeline(
    tools: &[ResolvedTool],
    content: &str,
    file_path: &Path,
    config: &FormattingConfig,
) -> Result<Vec<TextEdit>, String> {
    let timeout_ms = config.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let timeout = Duration::from_millis(timeout_ms);

    let mut current = content.to_string();

    for tool in tools {
        current = run_tool(tool, &current, file_path, timeout)?;
    }

    Ok(compute_edits(content, &current))
}

/// Execute the resolved formatting strategy and return `TextEdit`s.
///
/// This is the main entry point called by the `formatting()` handler.
pub(crate) fn execute_strategy(
    strategy: &FormattingStrategy,
    content: &str,
    file_path: &Path,
    config: &FormattingConfig,
    php_version: crate::types::PhpVersion,
) -> Result<Option<Vec<TextEdit>>, String> {
    match strategy {
        FormattingStrategy::Disabled => Ok(None),
        FormattingStrategy::External(tools) => {
            let edits = run_external_pipeline(tools, content, file_path, config)?;
            if edits.is_empty() {
                Ok(None)
            } else {
                Ok(Some(edits))
            }
        }
        FormattingStrategy::BuiltIn => {
            let mago_version = to_mago_php_version(php_version);
            let formatted = format_with_mago(content, mago_version)?;
            let edits = compute_edits(content, &formatted);
            if edits.is_empty() {
                Ok(None)
            } else {
                Ok(Some(edits))
            }
        }
    }
}

/// Run a single tool on the content and return the formatted string.
fn run_tool(
    tool: &ResolvedTool,
    content: &str,
    file_path: &Path,
    timeout: Duration,
) -> Result<String, String> {
    match tool.name {
        "php-cs-fixer" => run_php_cs_fixer(&tool.path, content, file_path, timeout),
        "phpcbf" => run_phpcbf(&tool.path, content, file_path, timeout),
        "pint" => run_pint(&tool.path, content, file_path, timeout),
        _ => Err(format!("Unknown formatting tool: {}", tool.name)),
    }
}

/// Run php-cs-fixer on a sibling temp file and return the formatted content.
///
/// Command: `<tool> fix --using-cache=no --quiet --no-interaction <tempfile>`
///
/// php-cs-fixer modifies the file in-place.  Exit code 0 means success.
fn run_php_cs_fixer(
    tool_path: &Path,
    content: &str,
    file_path: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let temp = write_sibling_temp_file(file_path, content)?;

    let result = run_command_with_timeout(
        Command::new(tool_path)
            .arg("fix")
            .arg("--using-cache=no")
            .arg("--quiet")
            .arg("--no-interaction")
            .arg(temp.path()),
        timeout,
    );

    let formatted = std::fs::read_to_string(temp.path())
        .map_err(|e| format!("Failed to read formatted output: {}", e))?;

    match result {
        Ok(status) => {
            // php-cs-fixer exit codes (bitmask):
            //   0 = OK
            //   1 = general error / PHP version issue
            //  16 = configuration error
            //  32 = fixer configuration error
            //  64 = exception
            if status.code == 0 {
                Ok(formatted)
            } else {
                Err(format!(
                    "php-cs-fixer exited with code {} (stderr: {})",
                    status.code,
                    status.stderr.trim()
                ))
            }
        }
        Err(e) => Err(e),
    }
}

/// Run Pint via stdin and return the formatted content.
///
/// Command: `<tool> --stdin-filename=<file_path>`
///
/// Pint reads from stdin and writes the formatted output to stdout
/// when `--stdin-filename` is provided.
fn run_pint(
    tool_path: &Path,
    content: &str,
    file_path: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let mut child = Command::new(tool_path)
        .arg(format!("--stdin-filename={}", file_path.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn pint: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(content.as_bytes())
            .map_err(|e| format!("Failed to write to pint stdin: {}", e))?;
    }

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    std::io::Read::read_to_string(&mut out, &mut stdout)
                        .map_err(|e| format!("Failed to read pint stdout: {}", e))?;
                }

                let code = status.code().unwrap_or(-1);
                if code == 0 {
                    return Ok(stdout);
                }

                let mut stderr = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut err, &mut stderr);
                }
                return Err(format!(
                    "pint exited with code {} (stderr: {})",
                    code,
                    stderr.trim()
                ));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Formatter timed out after {}ms",
                        timeout.as_millis()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Error waiting for pint: {}", e));
            }
        }
    }
}

/// Run phpcbf on a sibling temp file and return the formatted content.
///
/// Command: `<tool> --no-colors -q <tempfile>`
///
/// phpcbf modifies the file in-place.
fn run_phpcbf(
    tool_path: &Path,
    content: &str,
    file_path: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let temp = write_sibling_temp_file(file_path, content)?;

    let result = run_command_with_timeout(
        Command::new(tool_path)
            .arg("--no-colors")
            .arg("-q")
            .arg(temp.path()),
        timeout,
    );

    let formatted = std::fs::read_to_string(temp.path())
        .map_err(|e| format!("Failed to read formatted output: {}", e))?;

    match result {
        Ok(status) => {
            // phpcbf exit codes:
            //   0 = no fixes needed
            //   1 = fixes applied (success)
            //   2 = could not fix all errors
            //   3+ = operational error
            match status.code {
                0 | 1 => Ok(formatted),
                _ => Err(format!(
                    "phpcbf exited with code {} (stderr: {})",
                    status.code,
                    status.stderr.trim()
                )),
            }
        }
        Err(e) => Err(e),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Write content to a temporary file in the same directory as `original`
/// so that tool config discovery (which walks up from the file) works.
///
/// Returns a `NamedTempFile` whose destructor automatically removes the
/// file on drop.  The caller must keep it alive until after reading back
/// the formatted content.
fn write_sibling_temp_file(original: &Path, content: &str) -> Result<NamedTempFile, String> {
    let parent = original
        .parent()
        .ok_or_else(|| "Cannot determine parent directory of file".to_string())?;

    let mut temp = tempfile::Builder::new()
        .prefix(".phpantom-fmt-")
        .suffix(".php")
        .tempfile_in(parent)
        .map_err(|e| format!("Failed to create temp file in {}: {}", parent.display(), e))?;

    temp.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    temp.flush()
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;

    Ok(temp)
}

/// Result of running an external command.
struct CommandResult {
    /// Exit code (or -1 if the process was killed / no code available).
    code: i32,
    /// Captured stderr content.
    stderr: String,
}

/// Spawn a command, wait for it with a timeout, and return the result.
///
/// Stdout is suppressed.  Stderr is captured for error reporting.
fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn formatter: {}", e))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok()?;
                        Some(buf)
                    })
                    .unwrap_or_default();

                return Ok(CommandResult {
                    code: status.code().unwrap_or(-1),
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Formatter timed out after {}ms",
                        timeout.as_millis()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Error waiting for formatter: {}", e));
            }
        }
    }
}

/// Compute the `TextEdit`s needed to transform `original` into `formatted`.
///
/// Returns a single `TextEdit` that replaces the entire document.  Only
/// returns edits if the content actually changed.
fn compute_edits(original: &str, formatted: &str) -> Vec<TextEdit> {
    if original == formatted {
        return Vec::new();
    }

    let line_count = original.lines().count();
    let last_line_idx = if line_count == 0 { 0 } else { line_count - 1 };
    let last_line_len = original.lines().last().map_or(0, |l| l.len());

    let (end_line, end_char) = if original.ends_with('\n') {
        (last_line_idx + 1, 0)
    } else {
        (last_line_idx, last_line_len)
    };

    vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: end_line as u32,
                character: end_char as u32,
            },
        },
        new_text: formatted.to_string(),
    }]
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── compute_edits ───────────────────────────────────────────────

    #[test]
    fn compute_edits_no_change() {
        let content = "<?php\necho 'hello';\n";
        let edits = compute_edits(content, content);
        assert!(edits.is_empty());
    }

    #[test]
    fn compute_edits_with_change() {
        let original = "<?php\necho 'hello';\n";
        let formatted = "<?php\n\necho 'hello';\n";
        let edits = compute_edits(original, formatted);
        assert_eq!(edits.len(), 1);
        let edit = &edits[0];
        assert_eq!(edit.range.start.line, 0);
        assert_eq!(edit.range.start.character, 0);
        assert_eq!(edit.range.end.line, 2);
        assert_eq!(edit.range.end.character, 0);
        assert_eq!(edit.new_text, formatted);
    }

    #[test]
    fn compute_edits_empty_original() {
        let original = "";
        let formatted = "<?php\n";
        let edits = compute_edits(original, formatted);
        assert_eq!(edits.len(), 1);
        let edit = &edits[0];
        assert_eq!(edit.range.start.line, 0);
        assert_eq!(edit.range.start.character, 0);
        assert_eq!(edit.range.end.line, 0);
        assert_eq!(edit.range.end.character, 0);
    }

    #[test]
    fn compute_edits_no_trailing_newline() {
        let original = "<?php\necho 'hello';";
        let formatted = "<?php\necho 'hello';\n";
        let edits = compute_edits(original, formatted);
        assert_eq!(edits.len(), 1);
        let edit = &edits[0];
        assert_eq!(edit.range.end.line, 1);
        assert_eq!(edit.range.end.character, 13);
    }

    // ── resolve_strategy ────────────────────────────────────────────

    #[test]
    fn strategy_default_config_no_composer_is_builtin() {
        let config = FormattingConfig::default();
        let strategy = resolve_strategy(None, &config, None, None);
        assert!(matches!(strategy, FormattingStrategy::BuiltIn));
    }

    #[test]
    fn strategy_both_disabled() {
        let config = FormattingConfig {
            pint: Some(String::new()),
            php_cs_fixer: Some(String::new()),
            phpcbf: Some(String::new()),
            timeout: None,
        };
        let strategy = resolve_strategy(None, &config, None, None);
        assert!(matches!(strategy, FormattingStrategy::Disabled));
    }

    #[test]
    fn strategy_explicit_commands() {
        let config = FormattingConfig {
            pint: None,
            php_cs_fixer: Some("/usr/bin/php-cs-fixer".to_string()),
            phpcbf: Some("/usr/bin/phpcbf".to_string()),
            timeout: None,
        };
        let strategy = resolve_strategy(None, &config, None, None);
        match &strategy {
            FormattingStrategy::External(tools) => {
                assert_eq!(tools.len(), 2);
                assert_eq!(tools[0].name, "php-cs-fixer");
                assert_eq!(tools[0].path, PathBuf::from("/usr/bin/php-cs-fixer"));
                assert_eq!(tools[1].name, "phpcbf");
                assert_eq!(tools[1].path, PathBuf::from("/usr/bin/phpcbf"));
            }
            other => panic!("Expected External, got {:?}", other),
        }
    }

    #[test]
    fn strategy_one_explicit_one_disabled() {
        let config = FormattingConfig {
            pint: None,
            php_cs_fixer: Some("/usr/bin/php-cs-fixer".to_string()),
            phpcbf: Some(String::new()),
            timeout: None,
        };
        let strategy = resolve_strategy(None, &config, None, None);
        match &strategy {
            FormattingStrategy::External(tools) => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].name, "php-cs-fixer");
            }
            other => panic!("Expected External, got {:?}", other),
        }
    }

    #[test]
    fn strategy_require_dev_php_cs_fixer() {
        let dir = tempfile::tempdir().unwrap();
        let vendor_bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();

        let p = vendor_bin.join("php-cs-fixer");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require-dev": {
                    "friendsofphp/php-cs-fixer": "^3.0"
                }
            }))
            .unwrap();

        let config = FormattingConfig::default();
        let strategy = resolve_strategy(Some(dir.path()), &config, Some(&composer), None);
        match &strategy {
            FormattingStrategy::External(tools) => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].name, "php-cs-fixer");
                assert_eq!(tools[0].path, vendor_bin.join("php-cs-fixer"));
            }
            other => panic!("Expected External, got {:?}", other),
        }
    }

    #[test]
    fn strategy_require_dev_phpcodesniffer() {
        let dir = tempfile::tempdir().unwrap();
        let vendor_bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();

        let p = vendor_bin.join("phpcbf");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require-dev": {
                    "squizlabs/php_codesniffer": "^3.0"
                }
            }))
            .unwrap();

        let config = FormattingConfig::default();
        let strategy = resolve_strategy(Some(dir.path()), &config, Some(&composer), None);
        match &strategy {
            FormattingStrategy::External(tools) => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].name, "phpcbf");
                assert_eq!(tools[0].path, vendor_bin.join("phpcbf"));
            }
            other => panic!("Expected External, got {:?}", other),
        }
    }

    #[test]
    fn strategy_require_dev_both_tools() {
        let dir = tempfile::tempdir().unwrap();
        let vendor_bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();

        for name in &["php-cs-fixer", "phpcbf"] {
            let p = vendor_bin.join(name);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }

        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require-dev": {
                    "friendsofphp/php-cs-fixer": "^3.0",
                    "squizlabs/php_codesniffer": "^3.0"
                }
            }))
            .unwrap();

        let config = FormattingConfig::default();
        let strategy = resolve_strategy(Some(dir.path()), &config, Some(&composer), None);
        match &strategy {
            FormattingStrategy::External(tools) => {
                assert_eq!(tools.len(), 2);
                assert_eq!(tools[0].name, "php-cs-fixer");
                assert_eq!(tools[1].name, "phpcbf");
            }
            other => panic!("Expected External, got {:?}", other),
        }
    }

    #[test]
    fn strategy_require_dev_binary_missing_falls_back_to_builtin() {
        // require-dev lists the package but the binary is not installed.
        let dir = tempfile::tempdir().unwrap();
        let vendor_bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();

        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require-dev": {
                    "friendsofphp/php-cs-fixer": "^3.0"
                }
            }))
            .unwrap();

        let config = FormattingConfig::default();
        let strategy = resolve_strategy(Some(dir.path()), &config, Some(&composer), None);
        assert!(
            matches!(strategy, FormattingStrategy::BuiltIn),
            "Expected BuiltIn when binary is missing, got {:?}",
            strategy,
        );
    }

    #[test]
    fn strategy_explicit_overrides_require_dev() {
        let dir = tempfile::tempdir().unwrap();
        let vendor_bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();

        let p = vendor_bin.join("php-cs-fixer");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require-dev": {
                    "friendsofphp/php-cs-fixer": "^3.0"
                }
            }))
            .unwrap();

        // User explicitly set a different path.
        let config = FormattingConfig {
            pint: None,
            php_cs_fixer: Some("/opt/php-cs-fixer".to_string()),
            phpcbf: Some(String::new()),
            timeout: None,
        };
        let strategy = resolve_strategy(Some(dir.path()), &config, Some(&composer), None);
        match &strategy {
            FormattingStrategy::External(tools) => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].path, PathBuf::from("/opt/php-cs-fixer"));
            }
            other => panic!("Expected External, got {:?}", other),
        }
    }

    #[test]
    fn strategy_custom_bin_dir() {
        let dir = tempfile::tempdir().unwrap();
        let custom_bin = dir.path().join("bin");
        std::fs::create_dir_all(&custom_bin).unwrap();

        let p = custom_bin.join("php-cs-fixer");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require-dev": {
                    "friendsofphp/php-cs-fixer": "^3.0"
                }
            }))
            .unwrap();

        let config = FormattingConfig::default();
        let strategy = resolve_strategy(Some(dir.path()), &config, Some(&composer), Some("bin"));
        match &strategy {
            FormattingStrategy::External(tools) => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].path, custom_bin.join("php-cs-fixer"));
            }
            other => panic!("Expected External, got {:?}", other),
        }
    }

    #[test]
    fn strategy_custom_bin_dir_ignores_default_vendor_bin() {
        // Tools exist in vendor/bin but the project uses a custom bin
        // dir that does NOT contain them — should fall back to built-in.
        let dir = tempfile::tempdir().unwrap();
        let vendor_bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();
        let custom_bin = dir.path().join("custom-bin");
        std::fs::create_dir_all(&custom_bin).unwrap();

        let p = vendor_bin.join("php-cs-fixer");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require-dev": {
                    "friendsofphp/php-cs-fixer": "^3.0"
                }
            }))
            .unwrap();

        let config = FormattingConfig::default();
        let strategy = resolve_strategy(
            Some(dir.path()),
            &config,
            Some(&composer),
            Some("custom-bin"),
        );
        // php-cs-fixer is in vendor/bin but NOT in custom-bin.
        assert!(
            matches!(strategy, FormattingStrategy::BuiltIn),
            "Expected BuiltIn when custom bin dir doesn't have the tool, got {:?}",
            strategy,
        );
    }

    #[test]
    fn strategy_no_require_dev_no_config_is_builtin() {
        // composer.json exists but has no require-dev.
        let composer: crate::composer::ComposerPackage =
            serde_json::from_value(serde_json::json!({
                "require": {
                    "php": "^8.0"
                }
            }))
            .unwrap();
        let config = FormattingConfig::default();
        let strategy = resolve_strategy(None, &config, Some(&composer), None);
        assert!(matches!(strategy, FormattingStrategy::BuiltIn));
    }

    // ── format_with_mago ────────────────────────────────────────────

    #[test]
    fn mago_formats_simple_php() {
        let input = "<?php\necho   'hello' ;  \n";
        let result = format_with_mago(input, mago_php_version::PHPVersion::PHP84);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let formatted = result.unwrap();
        // The formatter should produce valid PHP with normalized spacing.
        assert!(formatted.starts_with("<?php"));
        assert!(formatted.contains("echo"));
    }

    #[test]
    fn mago_returns_error_for_unparseable_php() {
        let input = "<?php\nfunction { broken syntax";
        let result = format_with_mago(input, mago_php_version::PHPVersion::PHP84);
        assert!(result.is_err());
    }

    #[test]
    fn mago_preserves_already_formatted() {
        // A well-formatted snippet should round-trip cleanly.
        let input = "<?php\n\necho 'hello';\n";
        let result = format_with_mago(input, mago_php_version::PHPVersion::PHP84);
        assert!(result.is_ok());
        let formatted = result.unwrap();
        assert_eq!(formatted, input);
    }

    #[test]
    fn mago_reformats_messy_class() {
        let input = "<?php\n\nnamespace Demo;\nclass User\n{ public function foo(): string\n{\n    return \"1a11a\";}\n}\n";
        let result = format_with_mago(input, mago_php_version::PHPVersion::PHP84);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let formatted = result.unwrap();
        assert_ne!(
            formatted, input,
            "Formatter should have changed the messy input"
        );
        // The formatter should produce proper brace placement.
        assert!(
            formatted.contains("class User\n{"),
            "Expected class brace on next line, got:\n{}",
            formatted,
        );
    }

    // ── to_mago_php_version ─────────────────────────────────────────

    #[test]
    fn php_version_conversion() {
        let v = crate::types::PhpVersion { major: 8, minor: 4 };
        let mago = to_mago_php_version(v);
        assert_eq!(mago.major(), 8);
        assert_eq!(mago.minor(), 4);
        assert_eq!(mago.patch(), 0);
    }

    // ── sibling temp file ───────────────────────────────────────────

    #[test]
    fn write_sibling_temp_file_in_same_dir() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("MyClass.php");
        std::fs::write(&original, "<?php\n").unwrap();

        let content = "<?php\necho 'formatted';\n";
        let temp = write_sibling_temp_file(&original, content).unwrap();

        assert_eq!(temp.path().parent(), original.parent());
        let name = temp.path().file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(".phpantom-fmt-"));
        assert!(name.ends_with(".php"));

        let read_back = std::fs::read_to_string(temp.path()).unwrap();
        assert_eq!(read_back, content);

        // NamedTempFile auto-deletes on drop — no manual remove needed.
    }

    // ── execute_strategy with built-in ──────────────────────────────

    #[test]
    fn execute_builtin_returns_edits_for_unformatted_input() {
        let content = "<?php\necho   'hello' ;  \n";
        let config = FormattingConfig::default();
        let php_version = crate::types::PhpVersion { major: 8, minor: 4 };
        let file_path = PathBuf::from("/tmp/test.php");

        let result = execute_strategy(
            &FormattingStrategy::BuiltIn,
            content,
            &file_path,
            &config,
            php_version,
        );
        assert!(result.is_ok());
        let edits = result.unwrap();
        assert!(edits.is_some(), "Expected some edits for unformatted input");
    }

    #[test]
    fn execute_builtin_reformats_messy_class() {
        let content = "<?php\n\nnamespace Demo;\nclass User\n{ public function foo(): string\n{\n    return \"1a11a\";}\n}\n";
        let config = FormattingConfig::default();
        let php_version = crate::types::PhpVersion { major: 8, minor: 4 };
        let file_path = PathBuf::from("/tmp/sandbox.php");

        let result = execute_strategy(
            &FormattingStrategy::BuiltIn,
            content,
            &file_path,
            &config,
            php_version,
        );
        assert!(result.is_ok());
        let edits = result.unwrap();
        assert!(edits.is_some(), "Expected edits for messy class, got None");
        let edits = edits.unwrap();
        assert!(!edits.is_empty());
        // The replacement text should have proper PER-CS formatting.
        let new_text = &edits[0].new_text;
        assert!(
            new_text.contains("class User\n{"),
            "Expected class brace on next line, got:\n{}",
            new_text,
        );
    }

    #[test]
    fn execute_disabled_returns_none() {
        let content = "<?php\necho 'hello';\n";
        let config = FormattingConfig {
            pint: None,
            php_cs_fixer: Some(String::new()),
            phpcbf: Some(String::new()),
            timeout: None,
        };
        let php_version = crate::types::PhpVersion { major: 8, minor: 4 };
        let file_path = PathBuf::from("/tmp/test.php");

        let result = execute_strategy(
            &FormattingStrategy::Disabled,
            content,
            &file_path,
            &config,
            php_version,
        );
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
