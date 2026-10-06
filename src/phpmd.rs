//! PHPMD (PHP Mess Detector) proxy for code quality diagnostics.
//!
//! PHPantom can proxy diagnostics from PHPMD 3 by running
//! `phpmd analyze --format=json -` on the current buffer.  This
//! surfaces mess detection findings (unused code, excessive complexity,
//! `eval`/`exit` usage, naming) as LSP diagnostics.  PHPMD 2 has a
//! different command line and is not supported.
//!
//! ## Auto-detection
//!
//! When `command` is unset, PHPantom only runs PHPMD for a project that
//! has opted in: a PHPMD config file at the workspace root (one of
//! [`CONFIG_FILES`]) or a `ruleset` set in `.phpantom.toml`.  Without
//! either, PHPMD would apply every built-in rule, which is noise rather
//! than the project's own standard.  The binary is then resolved via
//! Composer's bin-dir, falling back to `$PATH`.  Set `command = ""` to
//! explicitly disable PHPMD.
//!
//! ## Configuration (`.phpantom.toml`)
//!
//! ```toml
//! [phpmd]
//! # Command/path for phpmd. When unset, auto-detected via
//! # Composer's bin-dir, then $PATH (only when the project has a
//! # PHPMD config file or `ruleset` is set). Set to "" to disable.
//! # command = "vendor/bin/phpmd"
//!
//! # Ruleset name or file. When unset, PHPMD uses the config file
//! # it auto-detects in the project root (phpmd.yml, phpmd.xml, ...).
//! # ruleset = "cleancode"
//!
//! # Maximum runtime in milliseconds before PHPMD is killed.
//! # Defaults to 30 000 ms (30 seconds).
//! # timeout = 30000
//! ```
//!
//! ## Output parsing
//!
//! Each violation maps to a diagnostic with the rule name as the code
//! (e.g. `UnusedLocalVariable`), which is also the name PHPMD's
//! `#[SuppressWarnings]` attribute and `@SuppressWarnings(PHPMD.…)`
//! annotation take.  The rule's documentation URL becomes the
//! diagnostic's code description.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tower_lsp::lsp_types::{CodeDescription, Diagnostic, DiagnosticSeverity, NumberOrString, Url};

use crate::config::PhpmdConfig;

/// PHPMD exit code for a run that found violations.
const EXIT_VIOLATIONS: i32 = 2;

/// The config file names PHPMD 3 auto-detects in its working directory.
///
/// One list, because the two questions it answers must not disagree: a
/// project whose config certifies the binary ([`resolve_phpmd`]) is the
/// same project whose config certifies a workspace-wide run
/// ([`has_project_config`]).
const CONFIG_FILES: [&str; 15] = [
    "phpmd.yml",
    "phpmd.yaml",
    "phpmd.json",
    "phpmd.xml",
    "phpmd.php",
    ".phpmd.yml",
    ".phpmd.yaml",
    ".phpmd.json",
    ".phpmd.xml",
    ".phpmd.php",
    "phpmd.yml.dist",
    "phpmd.yaml.dist",
    "phpmd.json.dist",
    "phpmd.xml.dist",
    "phpmd.php.dist",
];

// ── Tool resolution ─────────────────────────────────────────────────

/// A resolved PHPMD binary ready to invoke.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedPhpmd {
    /// Absolute or relative path to the binary.
    pub path: PathBuf,
}

/// Whether the project has its own PHPMD config file.
pub(crate) fn has_project_config(workspace_root: &Path) -> bool {
    CONFIG_FILES
        .iter()
        .any(|name| workspace_root.join(name).is_file())
}

/// Attempt to resolve the PHPMD binary from configuration and the
/// workspace environment.
///
/// Resolution rules:
/// - Config value `Some("")` (empty string) → disabled (`None`).
/// - Config value `Some(cmd)` → use `cmd` as-is (user override).
/// - Config value `None` → auto-detect, but only when the project has a
///   PHPMD config file or a `ruleset` is configured: try
///   `<bin_dir>/phpmd` under the workspace root, then search `$PATH`.
pub(crate) fn resolve_phpmd(
    workspace_root: Option<&Path>,
    config: &PhpmdConfig,
    bin_dir: Option<&str>,
) -> Option<ResolvedPhpmd> {
    match config.command.as_deref() {
        Some("") => None,
        Some(cmd) => Some(ResolvedPhpmd {
            path: PathBuf::from(cmd),
        }),
        None => {
            if config.ruleset.is_none() && !workspace_root.is_some_and(has_project_config) {
                return None;
            }
            crate::process::auto_detect_binary(workspace_root, bin_dir, "phpmd")
                .map(|path| ResolvedPhpmd { path })
        }
    }
}

// ── PHPMD execution ─────────────────────────────────────────────────

/// The arguments every PHPMD run shares.
fn base_command(resolved: &ResolvedPhpmd, workspace_root: &Path, config: &PhpmdConfig) -> Command {
    let mut cmd = Command::new(&resolved.path);
    cmd.arg("analyze")
        .arg("--format=json")
        .arg("--no-progress")
        .arg("--no-ansi")
        .current_dir(workspace_root);

    if let Some(ref ruleset) = config.ruleset {
        cmd.arg(format!("--ruleset={}", ruleset));
    }

    cmd
}

/// Run PHPMD on the given buffer content and return LSP diagnostics.
///
/// The buffer is fed to PHPMD on stdin (the `-` path argument).  PHPMD
/// runs from the workspace root so that it auto-detects the project's
/// config file.
pub(crate) fn run_phpmd(
    resolved: &ResolvedPhpmd,
    content: &str,
    workspace_root: &Path,
    config: &PhpmdConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    let timeout = Duration::from_millis(config.timeout_ms());

    let mut cmd = base_command(resolved, workspace_root, config);
    // The result cache is keyed by file path, and stdin has none of its
    // own: leave the project's cache to the project's own runs.
    cmd.arg("--no-cache").arg("-");

    let output = crate::process::run_command_with_timeout(
        &mut cmd,
        timeout,
        cancelled,
        "PHPMD",
        Some(content),
    )?;

    // PHPMD exit codes:
    //   0 = no violations found
    //   2 = violations found
    //   3 = a file could not be processed
    //   anything else = PHPMD itself failed
    //
    // Exit 3 is an error even though the report may list violations:
    // when the buffer fails to parse, PHPMD 3.0 reports the violations
    // of the *previous* stdin run it saw, which can be another file
    // entirely.  Keeping the last good result is the only safe answer.
    match output.code {
        0 => Ok(Vec::new()),
        EXIT_VIOLATIONS => parse_phpmd_json(&output.stdout),
        code => Err(format!(
            "PHPMD exited with code {} (stderr: {})",
            code,
            output.stderr.trim()
        )),
    }
}

/// Run PHPMD once over the whole project and return diagnostics
/// grouped by file path.
///
/// No path argument is passed, so PHPMD scans the `paths` from its own
/// config file (the caller checks [`has_project_config`] first).  The
/// project's result cache is left to the config, as a run from the
/// command line would.  Runs with an extended timeout.
pub(crate) fn run_phpmd_workspace(
    resolved: &ResolvedPhpmd,
    workspace_root: &Path,
    config: &PhpmdConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    let timeout = Duration::from_millis(
        config
            .timeout_ms()
            .saturating_mul(crate::process::WORKSPACE_TIMEOUT_FACTOR),
    );

    let mut cmd = base_command(resolved, workspace_root, config);

    let output = crate::process::run_command_with_timeout(
        &mut cmd,
        timeout,
        cancelled,
        "PHPMD (workspace)",
        None,
    )?;

    crate::process::workspace_run_result(&output, "PHPMD", &[EXIT_VIOLATIONS], false, |stdout| {
        parse_phpmd_json_workspace(stdout, workspace_root)
    })
}

// ── JSON output parsing ─────────────────────────────────────────────

/// Parse PHPMD's JSON report for a single stdin run into LSP
/// diagnostics.
///
/// PHPMD JSON format (with `--format=json`):
///
/// ```json
/// {
///   "version": "3.0.0",
///   "package": "phpmd",
///   "files": [
///     {
///       "file": "php://stdin",
///       "violations": [
///         {
///           "beginLine": 6,
///           "endLine": 6,
///           "description": "Avoid unused local variables such as '$x'.",
///           "rule": "UnusedLocalVariable",
///           "ruleSet": "Unused Code Rules",
///           "externalInfoUrl": "https://phpmd.org/rules/unusedcode.html#unusedlocalvariable",
///           "priority": 3
///         }
///       ]
///     }
///   ]
/// }
/// ```
///
/// A stdin run analyses exactly one file, so every violation belongs to
/// the buffer whatever name the entry carries.
fn parse_phpmd_json(json_str: &str) -> Result<Vec<Diagnostic>, String> {
    let output: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse PHPMD JSON: {}", e))?;

    Ok(output
        .get("files")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
        .filter_map(|file| file.get("violations").and_then(|v| v.as_array()))
        .flatten()
        .filter_map(parse_phpmd_violation)
        .collect())
}

/// Parse PHPMD's JSON report for a project-wide run into diagnostics
/// grouped by file path.
///
/// Same violation format as [`parse_phpmd_json`], keyed by each entry's
/// `file`.  Relative paths are resolved against the workspace root.
fn parse_phpmd_json_workspace(
    json_str: &str,
    workspace_root: &Path,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    let output: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse PHPMD JSON: {}", e))?;

    let mut by_file: std::collections::HashMap<PathBuf, Vec<Diagnostic>> =
        std::collections::HashMap::new();

    for file in output
        .get("files")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
    {
        let Some(path) = file.get("file").and_then(|p| p.as_str()) else {
            continue;
        };
        let Some(violations) = file.get("violations").and_then(|v| v.as_array()) else {
            continue;
        };

        let mut file_path = PathBuf::from(path);
        if file_path.is_relative() {
            file_path = workspace_root.join(file_path);
        }

        by_file
            .entry(file_path)
            .or_default()
            .extend(violations.iter().filter_map(parse_phpmd_violation));
    }

    by_file.retain(|_, diags| !diags.is_empty());
    Ok(by_file)
}

/// Parse a single PHPMD violation object into an LSP `Diagnostic`.
fn parse_phpmd_violation(violation: &serde_json::Value) -> Option<Diagnostic> {
    let message = violation.get("description")?.as_str()?;
    let line = violation
        .get("beginLine")
        .and_then(|l| l.as_u64())
        .unwrap_or(1);
    let lsp_line = line.saturating_sub(1) as u32;

    let code = violation
        .get("rule")
        .and_then(|r| r.as_str())
        .map(|r| NumberOrString::String(r.to_string()));

    let code_description = violation
        .get("externalInfoUrl")
        .and_then(|u| u.as_str())
        .and_then(|u| Url::parse(u).ok())
        .map(|href| CodeDescription { href });

    // A violation spans `beginLine..endLine`, which for a method- or
    // class-level rule (ExcessiveMethodLength, TooManyMethods) is the
    // whole declaration.  Underlining all of it would bury the code, so
    // only the line the violation starts on is marked, as for PHPStan.
    Some(Diagnostic {
        range: crate::process::full_line_range(lsp_line),
        severity: Some(DiagnosticSeverity::WARNING),
        code,
        code_description,
        source: Some("phpmd".to_string()),
        message: message.to_string(),
        related_information: None,
        tags: None,
        data: None,
    })
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const STDIN_REPORT: &str = r#"{
        "version": "3.0.0",
        "package": "phpmd",
        "timestamp": "2026-10-05T22:26:45+00:00",
        "files": [
            {
                "file": "php:\/\/stdin",
                "relativePath": "php:\/\/stdin",
                "violations": [
                    {
                        "beginLine": 6, "endLine": 6,
                        "package": null, "function": null, "class": null, "method": null,
                        "description": "Avoid unused local variables such as '$x'.",
                        "rule": "UnusedLocalVariable",
                        "ruleSet": "Unused Code Rules",
                        "externalInfoUrl": "https:\/\/phpmd.org\/rules\/unusedcode.html#unusedlocalvariable",
                        "priority": 3
                    },
                    {
                        "beginLine": 10, "endLine": 40,
                        "package": "App", "function": null, "class": "Probe", "method": "foo",
                        "description": "The method foo() has 31 lines of code.",
                        "rule": "ExcessiveMethodLength",
                        "ruleSet": "Code Size Rules",
                        "externalInfoUrl": "https:\/\/phpmd.org\/rules\/codesize.html#excessivemethodlength",
                        "priority": 3
                    }
                ]
            }
        ]
    }"#;

    #[test]
    fn parse_stdin_report() {
        let diags = parse_phpmd_json(STDIN_REPORT).unwrap();
        assert_eq!(diags.len(), 2);

        assert_eq!(
            diags[0].message,
            "Avoid unused local variables such as '$x'."
        );
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("UnusedLocalVariable".to_string()))
        );
        assert_eq!(diags[0].source.as_deref(), Some("phpmd"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            diags[0].code_description.as_ref().map(|d| d.href.as_str()),
            Some("https://phpmd.org/rules/unusedcode.html#unusedlocalvariable")
        );
        assert_eq!(diags[0].range.start.line, 5);
        assert_eq!(diags[0].range.end.character, u32::MAX);
    }

    #[test]
    fn multi_line_violation_marks_only_its_first_line() {
        let diags = parse_phpmd_json(STDIN_REPORT).unwrap();
        assert_eq!(diags[1].range.start.line, 9);
        assert_eq!(diags[1].range.end.line, 9);
    }

    #[test]
    fn parse_empty_report() {
        let json = r#"{"version": "3.0.0", "package": "phpmd", "files": []}"#;
        assert!(parse_phpmd_json(json).unwrap().is_empty());
    }

    #[test]
    fn parse_invalid_json() {
        assert!(parse_phpmd_json("not json").is_err());
    }

    #[test]
    fn missing_or_invalid_info_url_leaves_no_code_description() {
        let json = r#"{"files": [{"file": "php://stdin", "violations": [
            {"beginLine": 1, "description": "A", "rule": "R1"},
            {"beginLine": 2, "description": "B", "rule": "R2", "externalInfoUrl": "not a url"}
        ]}]}"#;
        let diags = parse_phpmd_json(json).unwrap();
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().all(|d| d.code_description.is_none()));
    }

    #[test]
    fn parse_workspace_report_groups_by_file() {
        let json = r#"{"files": [
            {"file": "/proj/app/A.php", "violations": [
                {"beginLine": 3, "description": "Bad A", "rule": "EvalExpression"}
            ]},
            {"file": "app/B.php", "violations": [
                {"beginLine": 8, "description": "Bad B", "rule": "ExitExpression"}
            ]},
            {"file": "/proj/app/Clean.php", "violations": []}
        ]}"#;

        let map = parse_phpmd_json_workspace(json, Path::new("/proj")).unwrap();
        // Clean files are dropped; relative paths resolve against the root.
        assert_eq!(map.len(), 2);
        assert_eq!(map[Path::new("/proj/app/A.php")][0].message, "Bad A");
        assert_eq!(map[Path::new("/proj/app/B.php")][0].range.start.line, 7);
    }

    // ── resolve_phpmd ───────────────────────────────────────────────

    #[test]
    fn resolve_disabled_when_empty_string() {
        let config = PhpmdConfig {
            command: Some(String::new()),
            ..Default::default()
        };
        assert!(resolve_phpmd(None, &config, None).is_none());
    }

    #[test]
    fn resolve_explicit_command() {
        let config = PhpmdConfig {
            command: Some("custom/phpmd".to_string()),
            ..Default::default()
        };
        let resolved = resolve_phpmd(None, &config, None).unwrap();
        assert_eq!(resolved.path, PathBuf::from("custom/phpmd"));
    }

    #[test]
    fn auto_detect_requires_a_project_config_or_ruleset() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("phpmd"), "").unwrap();

        let config = PhpmdConfig::default();
        assert!(resolve_phpmd(Some(dir.path()), &config, None).is_none());

        let with_ruleset = PhpmdConfig {
            ruleset: Some("cleancode".to_string()),
            ..Default::default()
        };
        assert_eq!(
            resolve_phpmd(Some(dir.path()), &with_ruleset, None)
                .unwrap()
                .path,
            bin.join("phpmd")
        );

        std::fs::write(dir.path().join("phpmd.yml"), "").unwrap();
        assert_eq!(
            resolve_phpmd(Some(dir.path()), &config, None).unwrap().path,
            bin.join("phpmd")
        );
    }

    #[test]
    fn project_config_detection_covers_dotted_and_dist_names() {
        for name in [".phpmd.yml", "phpmd.xml.dist"] {
            let dir = tempfile::tempdir().unwrap();
            assert!(!has_project_config(dir.path()));
            std::fs::write(dir.path().join(name), "").unwrap();
            assert!(has_project_config(dir.path()), "{name}");
        }
    }
}
