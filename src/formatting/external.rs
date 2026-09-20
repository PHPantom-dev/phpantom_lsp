//! External formatter runners: php-cs-fixer, Laravel Pint and phpcbf.
//!
//! Every tool runs through [`crate::process::run_command_with_timeout`],
//! which never inherits the server's stdin (so a child cannot steal bytes
//! from the JSON-RPC pipe) and drains stdout/stderr while the child is
//! alive.  A tool that rewrites its argument in place (php-cs-fixer,
//! phpcbf) works on a sibling temp file so its config discovery finds the
//! project rules; a tool that formats stdin (Pint) is told the real path
//! with `--stdin-filename` for the same reason.  Every tool runs with the
//! workspace root as its working directory: Pint reads `pint.json` from
//! there (and, for Blade, the project's `node_modules`), not from the
//! directory the language server happened to be started in.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use tempfile::NamedTempFile;

use crate::config::FormattingConfig;

use super::{DEFAULT_TIMEOUT_MS, ResolvedTool, Tool};

/// How a tool takes its input and hands back the result.
enum Invocation {
    /// Reads the content from stdin and writes the formatted result to
    /// stdout.
    Stdin,
    /// Rewrites the file it is given in place, so it runs on a sibling
    /// temp file that is read back afterwards.
    SiblingFile,
}

impl Tool {
    fn invocation(self) -> Invocation {
        match self {
            Tool::Pint => Invocation::Stdin,
            Tool::PhpCsFixer | Tool::Phpcbf => Invocation::SiblingFile,
        }
    }

    /// The command-line arguments, given the path of the file being
    /// formatted: the real path for a stdin tool, the sibling temp file
    /// for one that rewrites in place.
    fn arguments(self, file_path: &Path) -> Vec<OsString> {
        match self {
            Tool::Pint => vec![format!("--stdin-filename={}", file_path.display()).into()],
            Tool::PhpCsFixer => vec![
                "fix".into(),
                "--using-cache=no".into(),
                "--quiet".into(),
                "--no-interaction".into(),
                file_path.into(),
            ],
            Tool::Phpcbf => vec!["--no-colors".into(), "-q".into(), file_path.into()],
        }
    }

    /// Whether `code` is an exit code the tool uses for a successful run.
    fn succeeded(self, code: i32) -> bool {
        match self {
            Tool::Pint => code == 0,
            // php-cs-fixer exit codes (bitmask):
            //   0 = OK
            //   1 = general error / PHP version issue
            //  16 = configuration error
            //  32 = fixer configuration error
            //  64 = exception
            Tool::PhpCsFixer => code == 0,
            // phpcbf exit codes:
            //   0 = no fixes needed
            //   1 = fixes applied (success)
            //   2 = could not fix all errors
            //   3+ = operational error
            Tool::Phpcbf => matches!(code, 0 | 1),
        }
    }
}

/// Where and how long a tool runs.
struct Run<'a> {
    workspace_root: Option<&'a Path>,
    timeout: Duration,
    cancelled: &'a AtomicBool,
}

impl<'a> Run<'a> {
    fn new(
        workspace_root: Option<&'a Path>,
        config: &FormattingConfig,
        cancelled: &'a AtomicBool,
    ) -> Self {
        Self {
            workspace_root,
            timeout: Duration::from_millis(config.timeout.unwrap_or(DEFAULT_TIMEOUT_MS)),
            cancelled,
        }
    }
}

/// Run the external tool pipeline on `content` and return the result.
///
/// Each tool in the pipeline runs in sequence.  The output of one tool
/// becomes the input for the next.
///
/// `file_path` is the real path of the file on disk, used so that
/// sibling temp files land in the correct directory for tool config
/// discovery.
pub(super) fn run_external_pipeline(
    tools: &[ResolvedTool],
    content: &str,
    file_path: &Path,
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let run = Run::new(workspace_root, config, cancelled);
    let mut current = content.to_string();
    for tool in tools {
        current = run_tool(tool, &current, file_path, &run)?;
    }
    Ok(current)
}

/// Format a Blade template through Pint's `Pint/laravel_blade` rule.
///
/// `blade_flag` passes `--blade`, which turns the rule on for the run
/// when the project's `pint.json` does not.  Pint refuses the run (exit
/// code 1, nothing written) when the Node dependencies the rule needs
/// are not installed, which surfaces as an error rather than as output.
pub(super) fn run_pint_on_blade(
    tool: &ResolvedTool,
    content: &str,
    file_path: &Path,
    workspace_root: Option<&Path>,
    blade_flag: bool,
    config: &FormattingConfig,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let mut arguments = Tool::Pint.arguments(file_path);
    if blade_flag {
        arguments.push("--blade".into());
    }
    let run = Run::new(workspace_root, config, cancelled);
    let output = execute(tool, &arguments, Some(content), &run)?;
    Ok(output.stdout)
}

fn run_tool(
    tool: &ResolvedTool,
    content: &str,
    file_path: &Path,
    run: &Run<'_>,
) -> Result<String, String> {
    match tool.tool.invocation() {
        Invocation::Stdin => {
            let output = execute(tool, &tool.tool.arguments(file_path), Some(content), run)?;
            Ok(output.stdout)
        }
        Invocation::SiblingFile => {
            let temp = write_sibling_temp_file(file_path, content)?;
            let result = execute(tool, &tool.tool.arguments(temp.path()), None, run);
            // Read back before checking the outcome: phpcbf reports partial
            // fixes through a non-zero code and the file is what it wrote.
            let formatted = std::fs::read_to_string(temp.path())
                .map_err(|e| format!("Failed to read formatted output: {}", e))?;
            result.map(|_| formatted)
        }
    }
}

/// Run one tool and check its exit code.
fn execute(
    tool: &ResolvedTool,
    arguments: &[OsString],
    stdin: Option<&str>,
    run: &Run<'_>,
) -> Result<crate::process::CommandOutput, String> {
    // A relative path with a directory in it (`./tools/pint`) is relative
    // to the server's own working directory, which changing the child's
    // working directory must not move.
    let program = if tool.path.is_relative() && tool.path.components().count() > 1 {
        std::path::absolute(&tool.path).unwrap_or_else(|_| tool.path.clone())
    } else {
        tool.path.clone()
    };
    let mut command = Command::new(program);
    command.args(arguments);
    if let Some(root) = run.workspace_root {
        command.current_dir(root);
    }
    let output = crate::process::run_command_with_timeout(
        &mut command,
        run.timeout,
        run.cancelled,
        tool.tool.name(),
        stdin,
    )?;
    if tool.tool.succeeded(output.code) {
        Ok(output)
    } else {
        Err(format!(
            "{} exited with code {} (stderr: {})",
            tool.tool.name(),
            output.code,
            output.stderr.trim()
        ))
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
