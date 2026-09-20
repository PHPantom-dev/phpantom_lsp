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
//! 2. `vendor/bin/mago` under the workspace root, when `composer.json`
//!    depends on `carthage-software/mago` directly.
//! 3. `mago` on `$PATH`.
//!
//! Set `command = ""` to explicitly disable Mago.
//!
//! Which of Mago's two diagnostic commands run is decided separately,
//! from the tables the workspace `mago.toml` carries — see
//! [`enabled_services`], and [`analyzer_understands_laravel`] for the
//! extra condition `mago analyze` carries on a Laravel project.
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
//! # Whether to proxy `mago lint` / `mago analyze` diagnostics. When
//! # unset, each follows the matching table in mago.toml.
//! # lint = true
//! # analyze = false
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

use serde::Deserialize;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};

use crate::composer::ComposerPackage;
use crate::config::MagoConfig;
use crate::process::{auto_detect_binary, paths_match};
use crate::text_position::offset_to_position;

/// Composer package name Mago is distributed under.
const MAGO_PACKAGE: &str = "carthage-software/mago";

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
/// - Config value `Some(cmd)` → use `cmd` as-is (user override, and it
///   bypasses the `composer.json` check below, which is how a manually
///   installed Mago outside the Composer bin dir is wired up).
/// - Config value `None` → auto-detect:
///   - `<bin_dir>/mago` under the workspace root, but only when the
///     project depends on `carthage-software/mago` directly (`require`
///     or `require-dev`), so a binary pulled in as somebody else's
///     transitive dependency is not proxied as though the project used
///     it.
///   - otherwise `$PATH`, unconditionally — installing Mago globally
///     was a deliberate choice rather than leftover state.
pub(crate) fn resolve_mago(
    workspace_root: Option<&Path>,
    config: &MagoConfig,
    bin_dir: Option<&str>,
    composer_json: Option<&ComposerPackage>,
) -> Option<ResolvedMago> {
    match config.command.as_deref() {
        Some("") => None,
        Some(cmd) => Some(ResolvedMago {
            path: PathBuf::from(cmd),
        }),
        None => {
            // Only a project that depends on Mago has a vendored copy to
            // prefer over the one on `$PATH`.
            let depends_on_mago =
                composer_json.is_some_and(|pkg| crate::composer::has_dependency(pkg, MAGO_PACKAGE));
            auto_detect_binary(workspace_root.filter(|_| depends_on_mago), bin_dir, "mago")
                .map(|path| ResolvedMago { path })
        }
    }
}

// ── Service detection ───────────────────────────────────────────────

/// Which of Mago's two diagnostic commands PHPantom should proxy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MagoServices {
    /// Proxy `mago lint`.
    pub lint: bool,
    /// Proxy `mago analyze`.
    pub analyze: bool,
}

impl MagoServices {
    /// Whether neither command should run.
    pub fn none_enabled(&self) -> bool {
        !self.lint && !self.analyze
    }
}

/// What a workspace `mago.toml` says about how the project uses Mago.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct MagoTomlProbe {
    /// A `[linter]` table is present.
    linter: bool,
    /// An `[analyzer]` table is present.
    analyzer: bool,
    /// A `[formatter]` table is present.
    formatter: bool,
    /// The file wires up at least one extension.
    extension: bool,
}

/// The `mago.toml` tables PHPantom reads.
///
/// Every other key is ignored, so the probe stays valid as Mago's schema
/// grows.  It deliberately does not mirror Mago's own schema, which
/// rejects unknown fields: a key PHPantom has never heard of must not
/// stop it reading the ones it has.
#[derive(Deserialize)]
struct MagoToml {
    linter: Option<serde::de::IgnoredAny>,
    analyzer: Option<MagoTomlAnalyzer>,
    formatter: Option<serde::de::IgnoredAny>,
    #[serde(rename = "extension-hosts", default)]
    extension_hosts: std::collections::HashMap<String, MagoTomlExtensionHost>,
}

#[derive(Deserialize)]
struct MagoTomlAnalyzer {
    #[serde(default)]
    plugins: Vec<String>,
}

#[derive(Deserialize)]
struct MagoTomlExtensionHost {
    /// Mago starts a host unless it is switched off, so an entry that
    /// leaves this out counts as enabled.
    #[serde(default = "host_enabled_by_default")]
    enabled: bool,
}

fn host_enabled_by_default() -> bool {
    true
}

/// Which Mago diagnostics to proxy for a workspace.
///
/// `lint` and `analyze` in `.phpantom.toml`'s `[mago]` section win
/// outright.  Left unset, each follows the workspace `mago.toml`: a
/// `[linter]` table enables `mago lint`, an `[analyzer]` table enables
/// `mago analyze`.
///
/// Mago needs a `mago.toml` to run at all, so the tables that file
/// carries are also the record of what a project uses Mago for.  A file
/// holding a `[formatter]` table and nothing else belongs to a project
/// that formats with Mago and checks its code with something else, and
/// running `mago lint` and `mago analyze` at such a project reports code
/// nobody asked Mago about.
///
/// On a Laravel project `analyze` carries the extra condition described
/// in [`analyzer_understands_laravel`].
pub(crate) fn enabled_services(
    workspace_root: &Path,
    config: &MagoConfig,
    laravel: bool,
) -> MagoServices {
    if config.is_disabled() {
        return MagoServices::default();
    }

    let probe = probe_mago_toml(workspace_root);

    MagoServices {
        lint: config.lint.unwrap_or(probe.linter),
        analyze: config
            .analyze
            .unwrap_or(probe.analyzer && (!laravel || analyzer_understands_laravel(&probe))),
    }
}

/// Whether something could be teaching `mago analyze` about Laravel.
///
/// Mago's analyser has no built-in Laravel support, so on a Laravel
/// project it cannot see through Eloquent or the facades and reports
/// correct code in bulk: a `Collection` returned from a query looks
/// non-traversable to it, and every `foreach` over one is flagged.  The
/// gap is meant to be closed by an extension, which is a mechanism Mago
/// only grew in 1.47, and no Laravel extension exists yet.
///
/// A `mago.toml` that wires up no extension therefore has nothing that
/// could be supplying that knowledge, and this doubles as the version
/// check: Mago rejects a configuration file carrying keys it does not
/// know, so an `[extension-hosts]` table cannot appear in a file an
/// older Mago accepts.
///
/// An extension is counted as wired up when the file declares an enabled
/// extension host, or names a namespaced plugin (`vendor/name`, as in
/// `plugins = ["acme/laravel"]`).  Mago's own plugins are all bare names
/// (`stdlib`, `psl`, `flow-php`, `psr-container`), so they do not count:
/// none of them knows anything about Laravel.
fn analyzer_understands_laravel(probe: &MagoTomlProbe) -> bool {
    probe.extension
}

/// Read the workspace `mago.toml`.
///
/// A missing file means no services: Mago will not run without one.
fn probe_mago_toml(workspace_root: &Path) -> MagoTomlProbe {
    match std::fs::read_to_string(workspace_root.join("mago.toml")) {
        Ok(source) => parse_mago_toml(&source),
        Err(_) => MagoTomlProbe::default(),
    }
}

/// Read `mago.toml` source text.
///
/// A file Mago itself would reject says nothing about intent, so a parse
/// failure enables nothing.
fn parse_mago_toml(source: &str) -> MagoTomlProbe {
    let Ok(config) = toml::from_str::<MagoToml>(source) else {
        return MagoTomlProbe::default();
    };

    let has_enabled_host = config.extension_hosts.values().any(|host| host.enabled);
    let has_extension_plugin = config
        .analyzer
        .as_ref()
        .is_some_and(|analyzer| analyzer.plugins.iter().any(|name| name.contains('/')));

    MagoTomlProbe {
        linter: config.linter.is_some(),
        analyzer: config.analyzer.is_some(),
        formatter: config.formatter.is_some(),
        extension: has_enabled_host || has_extension_plugin,
    }
}

/// Whether the workspace `mago.toml` records Mago as the project's
/// formatter.
///
/// The same reading of that file as [`enabled_services`]: a `[formatter]`
/// table is what a project writes when it formats with Mago, and it is
/// deliberate in a way that a fixer shipped alongside a linter's ruleset is
/// not.  [`crate::formatting::resolve_strategy`] uses it to keep phpcbf from
/// taking over such a project.
pub(crate) fn formats_with_mago(workspace_root: &Path) -> bool {
    probe_mago_toml(workspace_root).formatter
}

// ── Mago execution ─────────────────────────────────────────────────

/// A per-file Mago command: [`run_mago_lint`] or [`run_mago_analyze`].
pub(crate) type MagoFileRunner = fn(
    &ResolvedMago,
    &str,
    &Path,
    &Path,
    &MagoConfig,
    &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String>;

/// Which Mago command to run on a single file, and how its findings
/// are labelled.
struct MagoCommand<'a> {
    /// The CLI subcommand, e.g. `lint`.
    subcommand: &'a str,
    /// Human-readable name for timeout and failure messages.
    label: &'a str,
    /// The `source` field every diagnostic it produces carries.
    source: &'a str,
    timeout_ms: u64,
}

/// Run one per-file Mago command on the given buffer content and return
/// LSP diagnostics.
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
fn run_mago_command(
    command: MagoCommand<'_>,
    resolved: &ResolvedMago,
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    let MagoCommand {
        subcommand,
        label,
        source,
        timeout_ms,
    } = command;
    let timeout = Duration::from_millis(timeout_ms);

    let mut cmd = Command::new(&resolved.path);
    cmd.arg(subcommand)
        .arg("--reporting-format")
        .arg("json")
        .arg("--stdin-input")
        .arg(file_path)
        .stdin(Stdio::piped())
        .current_dir(workspace_root);

    let file_path_str = file_path.to_string_lossy();
    let result = crate::process::run_command_with_timeout(
        &mut cmd,
        timeout,
        cancelled,
        label,
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
                match parse_mago_json(&result.stdout, content, &file_path_str, source) {
                    Ok(diags) => Ok(diags),
                    Err(_) => Ok(Vec::new()),
                }
            }
        }
        1 => parse_mago_json(&result.stdout, content, &file_path_str, source),
        _ => match parse_mago_json(&result.stdout, content, &file_path_str, source) {
            Ok(diags) if !diags.is_empty() => Ok(diags),
            _ => Err(format!(
                "{label} exited with code {} (stderr: {})",
                result.code,
                result.stderr.trim()
            )),
        },
    }
}

/// Run `mago lint` on the given buffer content. See [`run_mago_command`].
pub(crate) fn run_mago_lint(
    resolved: &ResolvedMago,
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    config: &MagoConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    run_mago_command(
        MagoCommand {
            subcommand: "lint",
            label: "Mago lint",
            source: "mago-lint",
            timeout_ms: config.lint_timeout_ms(),
        },
        resolved,
        content,
        file_path,
        workspace_root,
        cancelled,
    )
}

/// Run `mago analyze` on the given buffer content, the slower type-aware
/// analysis. See [`run_mago_command`].
pub(crate) fn run_mago_analyze(
    resolved: &ResolvedMago,
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    config: &MagoConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    run_mago_command(
        MagoCommand {
            subcommand: "analyze",
            label: "Mago analyze",
            source: "mago-analyze",
            timeout_ms: config.analyze_timeout_ms(),
        },
        resolved,
        content,
        file_path,
        workspace_root,
        cancelled,
    )
}

/// Run `mago lint` once over the whole project and return diagnostics
/// grouped by file path.
///
/// No `--stdin-input` and no path argument: Mago scans the source
/// paths from `mago.toml` (the caller checks for `mago.toml` first).
pub(crate) fn run_mago_lint_workspace(
    resolved: &ResolvedMago,
    workspace_root: &Path,
    config: &MagoConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    run_mago_workspace(
        resolved,
        workspace_root,
        "lint",
        config.lint_timeout_ms(),
        "mago-lint",
        cancelled,
    )
}

/// Run `mago analyze` once over the whole project and return
/// diagnostics grouped by file path.
pub(crate) fn run_mago_analyze_workspace(
    resolved: &ResolvedMago,
    workspace_root: &Path,
    config: &MagoConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    run_mago_workspace(
        resolved,
        workspace_root,
        "analyze",
        config.analyze_timeout_ms(),
        "mago-analyze",
        cancelled,
    )
}

/// Shared implementation for project-wide `mago lint` / `mago analyze`.
fn run_mago_workspace(
    resolved: &ResolvedMago,
    workspace_root: &Path,
    subcommand: &str,
    base_timeout_ms: u64,
    source_name: &str,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    let timeout = Duration::from_millis(
        base_timeout_ms.saturating_mul(crate::process::WORKSPACE_TIMEOUT_FACTOR),
    );

    let mut cmd = Command::new(&resolved.path);
    cmd.arg(subcommand)
        .arg("--reporting-format")
        .arg("json")
        .current_dir(workspace_root);

    let tool_name = format!("Mago {} (workspace)", subcommand);
    let result =
        crate::process::run_command_with_timeout(&mut cmd, timeout, cancelled, &tool_name, None)?;

    crate::process::workspace_run_result(&result, &tool_name, &[1], true, |stdout| {
        parse_mago_json_workspace(stdout, workspace_root, source_name)
    })
}

/// Parse Mago's JSON output into diagnostics grouped by file path.
///
/// Issues are attributed to the file of their first `Primary`
/// annotation.  Byte offsets are converted to positions using the
/// on-disk file content, read once per file and cached.  Issues whose
/// file cannot be read (deleted since the run started) are dropped.
fn parse_mago_json_workspace(
    json_str: &str,
    workspace_root: &Path,
    source_name: &str,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    let output: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse Mago JSON: {}", e))?;

    let mut by_file: std::collections::HashMap<PathBuf, Vec<Diagnostic>> =
        std::collections::HashMap::new();
    let mut content_cache: std::collections::HashMap<PathBuf, Option<String>> =
        std::collections::HashMap::new();

    if let Some(issues) = output.get("issues").and_then(|i| i.as_array()) {
        for issue in issues {
            let Some(path_str) = issue_primary_path(issue) else {
                continue;
            };
            let mut file_path = PathBuf::from(&path_str);
            if file_path.is_relative() {
                file_path = workspace_root.join(file_path);
            }

            let content = content_cache
                .entry(file_path.clone())
                .or_insert_with(|| std::fs::read_to_string(&file_path).ok());
            let Some(content) = content else {
                continue;
            };

            if let Some(diag) = parse_mago_issue(issue, content, &path_str, source_name) {
                by_file.entry(file_path).or_default().push(diag);
            }
        }
    }

    Ok(by_file)
}

/// The file path of an issue's first `Primary` annotation.
fn issue_primary_path(issue: &serde_json::Value) -> Option<String> {
    issue
        .get("annotations")?
        .as_array()?
        .iter()
        .find_map(|ann| {
            if ann.get("kind").and_then(|k| k.as_str()) != Some("Primary") {
                return None;
            }
            ann.get("span")?
                .get("file_id")?
                .get("path")?
                .as_str()
                .map(str::to_string)
        })
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

        let start_pos = offset_to_position(content, start_offset);
        let end_pos = offset_to_position(content, end_offset);

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

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_mago_json_workspace ───────────────────────────────────

    #[test]
    fn parse_workspace_json_reads_file_content_for_positions() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("A.php");
        std::fs::write(&file, "<?php\n$x = 1;\n").unwrap();

        let json = format!(
            r#"{{"issues": [
                {{"level": "Warning", "code": "some-rule", "message": "Msg",
                  "annotations": [{{"kind": "Primary", "span": {{
                      "file_id": {{"name": "A.php", "path": "{}"}},
                      "start": {{"offset": 6, "line": 2}},
                      "end": {{"offset": 8, "line": 2}}}}}}]}},
                {{"level": "Error", "code": "other-rule", "message": "Gone",
                  "annotations": [{{"kind": "Primary", "span": {{
                      "file_id": {{"name": "Missing.php", "path": "{}/Missing.php"}},
                      "start": {{"offset": 0, "line": 1}},
                      "end": {{"offset": 1, "line": 1}}}}}}]}}
            ]}}"#,
            file.display(),
            dir.path().display(),
        );

        let map = parse_mago_json_workspace(&json, dir.path(), "mago-lint").unwrap();
        // The issue for the deleted file is dropped.
        assert_eq!(map.len(), 1);
        let diags = &map[&file];
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "Msg");
        // Offset 6 is the start of line 2 in the on-disk content.
        assert_eq!(diags[0].range.start.line, 1);
        assert_eq!(diags[0].range.start.character, 0);
        assert_eq!(
            diags[0].source.as_deref(),
            Some("mago-lint"),
            "workspace results carry the same source as per-file runs"
        );
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

    /// A `composer.json` declaring a direct dependency on Mago.
    fn pkg_with_mago() -> ComposerPackage {
        r#"{"require-dev": {"carthage-software/mago": "^1.15"}}"#
            .parse()
            .unwrap()
    }

    /// Write an executable stub at `path`.
    fn write_stub_binary(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn resolve_disabled_when_empty_string() {
        let config = MagoConfig {
            command: Some(String::new()),
            ..MagoConfig::default()
        };
        let result = resolve_mago(None, &config, None, None);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_explicit_command() {
        let config = MagoConfig {
            command: Some("/usr/local/bin/mago".to_string()),
            ..MagoConfig::default()
        };
        // An explicit command bypasses the composer.json check.
        let result = resolve_mago(None, &config, None, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, PathBuf::from("/usr/local/bin/mago"));
    }

    #[test]
    fn resolve_auto_detect_vendor_bin() {
        let dir = tempfile::tempdir().unwrap();
        let mago = dir.path().join("vendor").join("bin").join("mago");
        write_stub_binary(&mago);

        let config = MagoConfig::default();
        let package = pkg_with_mago();
        let result = resolve_mago(Some(dir.path()), &config, None, Some(&package));
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, mago);
    }

    #[test]
    fn resolve_auto_detect_custom_bin_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mago = dir.path().join("tools").join("mago");
        write_stub_binary(&mago);

        let config = MagoConfig::default();
        let package = pkg_with_mago();
        let result = resolve_mago(Some(dir.path()), &config, Some("tools"), Some(&package));
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, mago);
    }

    #[test]
    fn resolve_auto_detect_skipped_without_composer_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let mago = dir.path().join("vendor").join("bin").join("mago");
        write_stub_binary(&mago);

        // A binary sits at vendor/bin/mago, but composer.json does not
        // depend on carthage-software/mago, so it arrived as somebody
        // else's transitive dependency (or was left behind). An
        // unrelated `mago` may still be on $PATH in some environments,
        // so only assert the vendor/bin one is excluded.
        let config = MagoConfig::default();
        let package: ComposerPackage = r#"{"require": {}}"#.parse().unwrap();
        let result = resolve_mago(Some(dir.path()), &config, None, Some(&package));
        assert_ne!(result.map(|r| r.path), Some(mago.clone()));

        // No composer.json at all behaves the same way.
        let result = resolve_mago(Some(dir.path()), &config, None, None);
        assert_ne!(result.map(|r| r.path), Some(mago));
    }

    #[test]
    fn resolve_no_binary_found() {
        let dir = tempfile::tempdir().unwrap();
        let config = MagoConfig::default();
        let package = pkg_with_mago();
        // No vendor/bin/mago, and PATH is unlikely to have it in test env.
        // This test may still find mago on PATH in some environments,
        // so we just verify it doesn't panic.
        let _ = resolve_mago(Some(dir.path()), &config, None, Some(&package));
    }

    // ── parse_mago_toml ─────────────────────────────────────────────

    #[test]
    fn formatter_only_config_enables_no_diagnostics() {
        let probe = parse_mago_toml("php-version = \"8.3\"\n\n[formatter]\nprint-width = 100\n");
        assert!(probe.formatter);
        assert!(!probe.linter);
        assert!(!probe.analyzer);
        assert!(!probe.extension);
    }

    #[test]
    fn linter_table_is_read() {
        let probe = parse_mago_toml("[linter]\nintegrations = [\"laravel\"]\n");
        assert!(probe.linter);
        assert!(!probe.analyzer);
    }

    #[test]
    fn linter_rules_subtable_counts_as_linter() {
        // `[linter.rules]` creates the `linter` table implicitly.
        let probe = parse_mago_toml("[linter.rules]\nhalstead = { enabled = false }\n");
        assert!(probe.linter);
        assert!(!probe.analyzer);
    }

    #[test]
    fn analyzer_table_is_read() {
        let probe = parse_mago_toml("[analyzer]\nanalyze-dead-code = true\n");
        assert!(!probe.linter);
        assert!(probe.analyzer);
        assert!(!probe.extension);
    }

    #[test]
    fn both_tables_are_read() {
        let probe = parse_mago_toml("[linter]\n\n[analyzer]\n");
        assert!(probe.linter);
        assert!(probe.analyzer);
    }

    #[test]
    fn malformed_config_configures_nothing() {
        assert_eq!(
            parse_mago_toml("[linter\nthis is not toml"),
            MagoTomlProbe::default()
        );
    }

    #[test]
    fn unknown_keys_do_not_stop_the_probe() {
        // Mago's own schema rejects unknown fields; PHPantom's must not,
        // or a key from a newer Mago would hide the tables it does read.
        let probe = parse_mago_toml("[analyzer]\nsome-future-key = 7\n\n[linter]\n");
        assert!(probe.linter);
        assert!(probe.analyzer);
    }

    // ── extension detection ─────────────────────────────────────────

    #[test]
    fn enabled_extension_host_counts_as_an_extension() {
        let probe = parse_mago_toml(
            "[analyzer]\n\n[extension-hosts.framework]\ncommand = [\"php\", \".mago/worker.php\"]\n",
        );
        assert!(probe.extension, "a host with no `enabled` key is enabled");
    }

    #[test]
    fn disabled_extension_host_does_not_count() {
        let probe = parse_mago_toml(
            "[analyzer]\n\n[extension-hosts.framework]\nenabled = false\ncommand = [\"php\", \"w.php\"]\n",
        );
        assert!(!probe.extension);
    }

    #[test]
    fn a_namespaced_plugin_counts_as_an_extension() {
        let probe = parse_mago_toml("[analyzer]\nplugins = [\"acme/laravel\"]\n");
        assert!(probe.extension);
    }

    #[test]
    fn magos_own_plugins_do_not_count_as_extensions() {
        // `stdlib`, `psl`, `flow-php` and `psr-container` ship with Mago
        // and none of them knows anything about Laravel. They also
        // predate extension support, so treating them as one would keep
        // `mago analyze` running on every Laravel project that ran
        // `mago init`.
        let probe = parse_mago_toml(
            "[analyzer]\nplugins = [\"stdlib\", \"psl\", \"flow-php\", \"psr-container\"]\n",
        );
        assert!(probe.analyzer);
        assert!(!probe.extension);
    }

    #[test]
    fn a_fully_specified_extension_host_is_read() {
        // Mago's documented example, verbatim, so the probe is pinned to
        // the real shape rather than the subset the tests above use.
        let probe = parse_mago_toml(
            r#"
[analyzer]
plugins = ["acme/laravel"]

[extension-hosts.framework]
enabled = true
command = ["php", ".mago/framework-worker.php"]
workers = 0
working-directory = "."
inherit-environment = true
environment = { APP_ENV = "analysis" }
maximum-payload-size = 67108864
request-timeout-ms = 30000
"#,
        );
        assert!(probe.analyzer);
        assert!(probe.extension);
    }

    // ── enabled_services ────────────────────────────────────────────

    /// Write `mago.toml` into a fresh temp workspace.
    fn workspace_with(mago_toml: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mago.toml"), mago_toml).unwrap();
        dir
    }

    #[test]
    fn enabled_services_reads_workspace_config() {
        let dir = workspace_with("[analyzer]\n");
        let services = enabled_services(dir.path(), &MagoConfig::default(), false);
        assert!(!services.lint);
        assert!(services.analyze);
    }

    #[test]
    fn enabled_services_none_without_mago_toml() {
        let dir = tempfile::tempdir().unwrap();
        assert!(enabled_services(dir.path(), &MagoConfig::default(), false).none_enabled());
    }

    #[test]
    fn analyze_is_off_on_laravel_without_an_extension() {
        let dir = workspace_with("[linter]\n\n[analyzer]\n");
        let services = enabled_services(dir.path(), &MagoConfig::default(), true);
        assert!(
            !services.analyze,
            "no extension can be teaching the analyser about Laravel"
        );
        assert!(
            services.lint,
            "the linter has a Laravel integration, so it is unaffected"
        );
    }

    #[test]
    fn analyze_is_on_on_laravel_with_an_extension() {
        let dir =
            workspace_with("[analyzer]\n\n[extension-hosts.framework]\ncommand = [\"php\"]\n");
        assert!(enabled_services(dir.path(), &MagoConfig::default(), true).analyze);
    }

    #[test]
    fn analyze_is_on_off_laravel_without_an_extension() {
        let dir = workspace_with("[analyzer]\n");
        assert!(enabled_services(dir.path(), &MagoConfig::default(), false).analyze);
    }

    #[test]
    fn explicit_analyze_overrides_the_laravel_condition() {
        let dir = workspace_with("[analyzer]\n");
        let on = MagoConfig {
            analyze: Some(true),
            ..MagoConfig::default()
        };
        assert!(enabled_services(dir.path(), &on, true).analyze);
    }

    #[test]
    fn enabled_services_honours_explicit_overrides() {
        let dir = workspace_with("[linter]\n\n[analyzer]\n");

        // Both configured in mago.toml, but turned off in .phpantom.toml.
        let off = MagoConfig {
            lint: Some(false),
            analyze: Some(false),
            ..MagoConfig::default()
        };
        assert!(enabled_services(dir.path(), &off, false).none_enabled());

        // And on for a formatter-only mago.toml.
        std::fs::write(dir.path().join("mago.toml"), "[formatter]\n").unwrap();
        let on = MagoConfig {
            lint: Some(true),
            analyze: Some(true),
            ..MagoConfig::default()
        };
        let services = enabled_services(dir.path(), &on, false);
        assert!(services.lint);
        assert!(services.analyze);
    }

    #[test]
    fn enabled_services_none_when_mago_disabled() {
        let dir = workspace_with("[linter]\n\n[analyzer]\n");

        // `command = ""` wins over both mago.toml and the toggles.
        let config = MagoConfig {
            command: Some(String::new()),
            lint: Some(true),
            analyze: Some(true),
            ..MagoConfig::default()
        };
        assert!(enabled_services(dir.path(), &config, false).none_enabled());
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
            lint_timeout: Some(15_000),
            ..MagoConfig::default()
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
            analyze_timeout: Some(120_000),
            ..MagoConfig::default()
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
