//! External formatter runners: php-cs-fixer, Laravel Pint and phpcbf.
//!
//! Every tool runs through [`crate::process::run_command_with_timeout`],
//! which never inherits the server's stdin (so a child cannot steal bytes
//! from the JSON-RPC pipe) and drains stdout/stderr while the child is
//! alive.  File-based tools (php-cs-fixer, phpcbf) work on a sibling
//! temp file so their config discovery finds the project rules; Pint is
//! fed through stdin with `--stdin-filename`.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use tempfile::NamedTempFile;
use tower_lsp::lsp_types::TextEdit;

use crate::config::FormattingConfig;

use super::{DEFAULT_TIMEOUT_MS, ResolvedTool, compute_edits};

/// Run the external tool pipeline on `content` and return `TextEdit`s.
///
/// Each tool in the pipeline runs in sequence.  The output of one tool
/// becomes the input for the next.  The final result is diffed against
/// the original content to produce edits.
///
/// `file_path` is the real path of the file on disk, used so that
/// sibling temp files land in the correct directory for tool config
/// discovery.
pub(super) fn run_external_pipeline(
    tools: &[ResolvedTool],
    content: &str,
    file_path: &Path,
    config: &FormattingConfig,
    cancelled: &AtomicBool,
) -> Result<Vec<TextEdit>, String> {
    let timeout_ms = config.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let timeout = Duration::from_millis(timeout_ms);

    let mut current = content.to_string();

    for tool in tools {
        current = run_tool(tool, &current, file_path, timeout, cancelled)?;
    }

    Ok(compute_edits(content, &current))
}

fn run_tool(
    tool: &ResolvedTool,
    content: &str,
    file_path: &Path,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    match tool.name {
        "php-cs-fixer" => run_php_cs_fixer(&tool.path, content, file_path, timeout, cancelled),
        "phpcbf" => run_phpcbf(&tool.path, content, file_path, timeout, cancelled),
        "pint" => run_pint(&tool.path, content, file_path, timeout, cancelled),
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
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let temp = write_sibling_temp_file(file_path, content)?;

    let result = crate::process::run_command_with_timeout(
        Command::new(tool_path)
            .arg("fix")
            .arg("--using-cache=no")
            .arg("--quiet")
            .arg("--no-interaction")
            .arg(temp.path()),
        timeout,
        cancelled,
        "php-cs-fixer",
        None,
    );

    let formatted = std::fs::read_to_string(temp.path())
        .map_err(|e| format!("Failed to read formatted output: {}", e))?;

    match result {
        Ok(output) => {
            // php-cs-fixer exit codes (bitmask):
            //   0 = OK
            //   1 = general error / PHP version issue
            //  16 = configuration error
            //  32 = fixer configuration error
            //  64 = exception
            if output.code == 0 {
                Ok(formatted)
            } else {
                Err(format!(
                    "php-cs-fixer exited with code {} (stderr: {})",
                    output.code,
                    output.stderr.trim()
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
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let output = crate::process::run_command_with_timeout(
        Command::new(tool_path).arg(format!("--stdin-filename={}", file_path.display())),
        timeout,
        cancelled,
        "pint",
        Some(content),
    )?;

    if output.code == 0 {
        Ok(output.stdout)
    } else {
        Err(format!(
            "pint exited with code {} (stderr: {})",
            output.code,
            output.stderr.trim()
        ))
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
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let temp = write_sibling_temp_file(file_path, content)?;

    let result = crate::process::run_command_with_timeout(
        Command::new(tool_path)
            .arg("--no-colors")
            .arg("-q")
            .arg(temp.path()),
        timeout,
        cancelled,
        "phpcbf",
        None,
    );

    let formatted = std::fs::read_to_string(temp.path())
        .map_err(|e| format!("Failed to read formatted output: {}", e))?;

    match result {
        Ok(output) => {
            // phpcbf exit codes:
            //   0 = no fixes needed
            //   1 = fixes applied (success)
            //   2 = could not fix all errors
            //   3+ = operational error
            match output.code {
                0 | 1 => Ok(formatted),
                _ => Err(format!(
                    "phpcbf exited with code {} (stderr: {})",
                    output.code,
                    output.stderr.trim()
                )),
            }
        }
        Err(e) => Err(e),
    }
}

/// Write content to a temporary file in the same directory as `original`
/// so that tool config discovery (which walks up from the file) works.
///
/// Returns a `NamedTempFile` whose destructor automatically removes the
/// file on drop.  The caller must keep it alive until after reading back
/// the formatted content.
pub(super) fn write_sibling_temp_file(
    original: &Path,
    content: &str,
) -> Result<NamedTempFile, String> {
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
