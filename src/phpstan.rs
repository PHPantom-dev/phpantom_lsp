//! PHPStan proxy for external static analysis diagnostics.
//!
//! PHPantom does not ship a static analyser.  Instead, it can proxy
//! diagnostics from PHPStan by running it in "editor mode" on the
//! unsaved buffer content.
//!
//! ## Editor mode
//!
//! PHPStan 2.1.17+ / 1.12.27+ supports `--tmp-file` and `--instead-of`
//! CLI options.  The idea: write the editor buffer to a temp file, then
//! tell PHPStan to analyse the project as if the original file had the
//! temp file's contents.  This gives full project-aware analysis with
//! proper result-cache behaviour and no side effects.
//!
//! ## Configuration (`.phpantom.toml`)
//!
//! ```toml
//! [phpstan]
//! # Command/path for phpstan. When unset, auto-detected: Composer's
//! # bin-dir (default vendor/bin) if the workspace root has a PHPStan
//! # config file, or composer.json depends directly on phpstan/phpstan or
//! # on a PHPStan extension that understands Laravel, then $PATH
//! # regardless of composer.json. A Laravel application that has
//! # installed neither such an extension nor a config file gets no
//! # PHPStan at all, since plain PHPStan misreads the framework.
//! # Set to "" to disable.
//! # command = "vendor/bin/phpstan"
//!
//! # Memory limit passed to PHPStan (default: "1G").
//! # memory-limit = "2G"
//!
//! # Maximum runtime in milliseconds before PHPStan is killed.
//! # Defaults to 60 000 ms (60 seconds).
//! # timeout = 60000
//! ```
//!
//! ## Output parsing
//!
//! PHPStan is invoked with `--error-format=json` and `--no-progress`.
//! The JSON output is parsed to extract file-level errors which are
//! converted to LSP `Diagnostic` values.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tempfile::NamedTempFile;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::config::PhpStanConfig;
use crate::process::{auto_detect_binary, paths_match};

/// Default PHPStan timeout in milliseconds (60 seconds).
const DEFAULT_TIMEOUT_MS: u64 = 60_000;

// ── Tool resolution ─────────────────────────────────────────────────

/// A resolved PHPStan binary ready to invoke.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedPhpStan {
    /// Absolute or relative path to the binary.
    pub path: PathBuf,
}

/// The config file names PHPStan itself looks for, in its own precedence
/// order.
///
/// One list, because the two questions it answers must not disagree: a
/// project whose config certifies a workspace-wide run
/// ([`has_project_config`]) is the same project whose config certifies the
/// vendored binary ([`resolve_phpstan`]).
const CONFIG_FILES: [&str; 3] = ["phpstan.neon", "phpstan.neon.dist", "phpstan.dist.neon"];

/// Attempt to resolve the PHPStan binary from configuration and the
/// workspace environment.
///
/// Resolution rules:
/// - Config value `Some("")` (empty string) → disabled (`None`).
/// - Config value `Some(cmd)` → use `cmd` as-is (user override, bypasses
///   the `composer.json` check below entirely — this is how a manually
///   installed PHPStan, e.g. a `.phar` outside the Composer bin dir, is
///   wired up).
/// - Config value `None` → auto-detect:
///   - nothing at all on a Laravel application that has installed no
///     Laravel-aware PHPStan extension and hand-authored no config. Plain
///     PHPStan does not
///     understand Eloquent magic, facades, or container bindings, so such a
///     project is left alone rather than run through an analyser that would
///     misread its own framework and report false positives, the same
///     reasoning that gates Mago's analyzer on
///     `analyzer_understands_laravel`. This is a refusal rather than a
///     missing piece of evidence, so it applies to every binary, `$PATH`
///     included. A library that merely requires an Illuminate component has
///     no framework to misread and is not covered by it.
///   - `<bin_dir>/phpstan` under the workspace root, but only when one of
///     the following certifies the project actually uses PHPStan:
///     - the workspace root has one of [`CONFIG_FILES`]. A project that
///       hand-authors a PHPStan config wants PHPStan run, independent of
///       what `composer.json` declares.
///     - otherwise, the project depends directly (`require` or
///       `require-dev`) on `phpstan/phpstan`, or on one of the PHPStan
///       extensions that understand Laravel
///       ([`crate::composer::has_laravel_aware_phpstan`]). This avoids
///       proxying to a stale `vendor/bin/phpstan` left behind by a project
///       that no longer depends on either.
///   - otherwise `$PATH` — a `phpstan` the user installed globally was a
///     deliberate choice, not leftover state, so the
///     `composer.json`/config-file evidence does not apply to it.
///   - otherwise `None`.
pub(crate) fn resolve_phpstan(
    workspace_root: Option<&Path>,
    config: &PhpStanConfig,
    bin_dir: Option<&str>,
    composer_json: Option<&crate::composer::ComposerPackage>,
) -> Option<ResolvedPhpStan> {
    match config.command.as_deref() {
        // Explicitly disabled.
        Some("") => None,
        // User-provided command.
        Some(cmd) => Some(ResolvedPhpStan {
            path: PathBuf::from(cmd),
        }),
        // Auto-detect.
        None => {
            let has_phpstan_config = workspace_root.is_some_and(has_project_config);
            let has_laravel_extension =
                composer_json.is_some_and(crate::composer::has_laravel_aware_phpstan);

            // A Laravel application with no Laravel-aware extension is
            // refused PHPStan outright: a global binary would misread the
            // framework exactly the way the vendored one would, and
            // additionally lacks the project's own extensions.
            if !has_phpstan_config
                && !has_laravel_extension
                && composer_json.is_some_and(crate::composer::is_laravel_application)
            {
                return None;
            }

            let depends_on_phpstan = has_phpstan_config
                || has_laravel_extension
                || composer_json
                    .is_some_and(|pkg| crate::composer::has_dependency(pkg, "phpstan/phpstan"));

            // Only a project that depends on PHPStan has a vendored copy
            // to prefer over the one on `$PATH`.
            auto_detect_binary(
                workspace_root.filter(|_| depends_on_phpstan),
                bin_dir,
                "phpstan",
            )
            .map(|path| ResolvedPhpStan { path })
        }
    }
}

// ── PHPStan execution ───────────────────────────────────────────────

/// Run PHPStan in editor mode on the given buffer content and return
/// LSP diagnostics.
///
/// `file_path` is the real path of the file on disk (used for the
/// `--instead-of` flag).  `content` is the current editor buffer
/// (which may differ from the on-disk version).
///
/// `workspace_root` is needed to run PHPStan from the project root
/// directory so that it picks up `phpstan.neon` / `phpstan.neon.dist`.
pub(crate) fn run_phpstan(
    resolved: &ResolvedPhpStan,
    content: &str,
    file_path: &Path,
    workspace_root: &Path,
    config: &PhpStanConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Diagnostic>, String> {
    let timeout_ms = config.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let timeout = Duration::from_millis(timeout_ms);
    let memory_limit = config.memory_limit.as_deref().unwrap_or("1G");

    // Build the PHPStan command.
    //
    // The file path is passed as a positional argument so that PHPStan
    // only analyses this single file, not the entire project.  Without
    // it, PHPStan would analyse all paths from phpstan.neon, which can
    // take minutes on large codebases.
    let mut cmd = Command::new(&resolved.path);
    cmd.arg("analyse")
        .arg("--error-format=json")
        .arg("--no-progress")
        .arg("--no-ansi")
        .arg(format!("--memory-limit={}", memory_limit));

    // Only enter editor mode (`--tmp-file` / `--instead-of`) when the
    // buffer differs from what is on disk. When they are byte-identical
    // we analyse the real file directly, so PHPStan sees it at its real
    // path. This matters for rules that inspect `$scope->getFile()`
    // (e.g. the Laravel extensions' env-outside-config rule): in editor mode PHPStan
    // analyses the temp file under the temp file's OWN path and only
    // swaps the path back to the real file when formatting errors, so a
    // temp file outside the real file's directory makes such rules
    // misfire. Keeping the temp file alive (its destructor removes it on
    // drop) until after PHPStan finishes; `None` in the direct path.
    let on_disk = std::fs::read(file_path).ok();
    let buffer_matches_disk = on_disk
        .as_deref()
        .map(|bytes| bytes == content.as_bytes())
        .unwrap_or(false);

    let tmp = if buffer_matches_disk {
        None
    } else {
        let tmp = write_temp_file(file_path, content)?;
        cmd.arg(format!("--tmp-file={}", tmp.path().display()))
            .arg(format!("--instead-of={}", file_path.display()));
        Some(tmp)
    };

    cmd.arg(file_path).current_dir(workspace_root);

    let result =
        crate::process::run_command_with_timeout(&mut cmd, timeout, cancelled, "PHPStan", None);

    // NamedTempFile auto-deletes on drop — but we keep `tmp` alive
    // until after we've consumed the command output.
    let _ = &tmp;

    match result {
        Ok(output) => {
            // PHPStan exit codes:
            //   0 = no errors found
            //   1 = errors found (this is the normal "has diagnostics" case)
            //   2+ = internal error / misconfiguration
            match output.code {
                0 => Ok(Vec::new()),
                1 => parse_phpstan_json(&output.stdout, file_path),
                _ => {
                    // For exit code 2+, check if there's still usable JSON
                    // output (PHPStan sometimes returns code 2 with partial
                    // results).  If parsing fails, report the error.
                    match parse_phpstan_json(&output.stdout, file_path) {
                        Ok(diags) if !diags.is_empty() => Ok(diags),
                        _ => Err(format!(
                            "PHPStan exited with code {} (stderr: {})",
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

/// Project-wide runs multiply the per-file timeout by this factor:
/// analysing a whole codebase legitimately takes far longer than the
/// Whether the project has its own PHPStan configuration file.
///
/// A project-wide run is only attempted when one exists: without it,
/// PHPStan has no `paths` setting and we would have to guess which
/// directories to analyse (risking a full vendor scan).  A hand-authored
/// config is also evidence that the project wants PHPStan run at all,
/// independent of what `composer.json` declares, which is what
/// [`resolve_phpstan`] reads it for.
pub(crate) fn has_project_config(workspace_root: &Path) -> bool {
    CONFIG_FILES
        .iter()
        .any(|name| workspace_root.join(name).is_file())
}

/// Run PHPStan once over the whole project and return diagnostics
/// grouped by file path.
///
/// No path argument is passed, so PHPStan analyses the `paths`
/// configured in its own configuration file (the caller checks
/// [`has_project_config`] first).  Runs with an extended timeout
/// ([`crate::process::WORKSPACE_TIMEOUT_FACTOR`] × the per-file timeout).
pub(crate) fn run_phpstan_workspace(
    resolved: &ResolvedPhpStan,
    workspace_root: &Path,
    config: &PhpStanConfig,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    let timeout_ms = config
        .timeout
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .saturating_mul(crate::process::WORKSPACE_TIMEOUT_FACTOR);
    let timeout = Duration::from_millis(timeout_ms);
    let memory_limit = config.memory_limit.as_deref().unwrap_or("1G");

    let mut cmd = Command::new(&resolved.path);
    cmd.arg("analyse")
        .arg("--error-format=json")
        .arg("--no-progress")
        .arg("--no-ansi")
        .arg(format!("--memory-limit={}", memory_limit))
        .current_dir(workspace_root);

    let output = crate::process::run_command_with_timeout(
        &mut cmd,
        timeout,
        cancelled,
        "PHPStan (workspace)",
        None,
    )?;

    crate::process::workspace_run_result(&output, "PHPStan", &[1], false, |stdout| {
        parse_phpstan_json_workspace(stdout, workspace_root)
    })
}

/// Parse PHPStan's JSON output into diagnostics grouped by file path.
///
/// Same message format as [`parse_phpstan_json`], but instead of
/// filtering to one file, every file entry is kept.  Relative paths
/// are resolved against the workspace root.  Top-level errors
/// (configuration issues) have no file to attach to and are dropped.
fn parse_phpstan_json_workspace(
    json_str: &str,
    workspace_root: &Path,
) -> Result<std::collections::HashMap<PathBuf, Vec<Diagnostic>>, String> {
    let output: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse PHPStan JSON: {}", e))?;

    let mut by_file: std::collections::HashMap<PathBuf, Vec<Diagnostic>> =
        std::collections::HashMap::new();

    if let Some(files) = output.get("files").and_then(|f| f.as_object()) {
        for (path, file_data) in files {
            // PHPStan may append a trait context to the key, e.g.
            // "/path/File.php (in context of class App\\Foo)".
            let clean_path = path.split(" (in context of").next().unwrap_or(path);
            let mut file_path = PathBuf::from(clean_path);
            if file_path.is_relative() {
                file_path = workspace_root.join(file_path);
            }

            if let Some(messages) = file_data.get("messages").and_then(|m| m.as_array()) {
                let diags = by_file.entry(file_path).or_default();
                for msg in messages {
                    if let Some(diag) = parse_phpstan_message(msg) {
                        diags.push(diag);
                    }
                }
            }
        }
    }

    by_file.retain(|_, diags| !diags.is_empty());
    Ok(by_file)
}

// ── JSON output parsing ─────────────────────────────────────────────

/// Parse PHPStan's JSON output into LSP diagnostics.
///
/// PHPStan JSON format (with `--error-format=json`):
///
/// ```json
/// {
///   "totals": { "errors": 0, "file_errors": 2 },
///   "files": {
///     "/path/to/file.php": {
///       "errors": 2,
///       "messages": [
///         {
///           "message": "...",
///           "line": 42,
///           "ignorable": true,
///           "identifier": "argument.type"
///         }
///       ]
///     }
///   },
///   "errors": []
/// }
/// ```
///
/// We extract messages for the file being edited (matching by path)
/// and also include top-level `errors` (configuration/internal errors).
fn parse_phpstan_json(json_str: &str, file_path: &Path) -> Result<Vec<Diagnostic>, String> {
    let output: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse PHPStan JSON: {}", e))?;

    let mut diagnostics = Vec::new();

    // Extract file-level errors.
    if let Some(files) = output.get("files").and_then(|f| f.as_object()) {
        // PHPStan keys files by their real path. The --tmp-file flag
        // causes PHPStan to report errors under the *original* file
        // path (the --instead-of path), not the temp file path.
        // We need to match against the original file path.
        let file_path_str = file_path.to_string_lossy();

        for (path, file_data) in files {
            // Match the file: PHPStan normalizes to absolute paths,
            // so compare by checking if either path ends with the other
            // or if they match exactly.
            if !paths_match(path, &file_path_str) {
                continue;
            }

            if let Some(messages) = file_data.get("messages").and_then(|m| m.as_array()) {
                for msg in messages {
                    if let Some(diag) = parse_phpstan_message(msg) {
                        diagnostics.push(diag);
                    }
                }
            }
        }
    }

    // Extract top-level errors (configuration issues, etc.).
    if let Some(errors) = output.get("errors").and_then(|e| e.as_array()) {
        for error in errors {
            if let Some(error_str) = error.as_str() {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: 0,
                        },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String("phpstan".to_string())),
                    code_description: None,
                    source: Some("phpstan".to_string()),
                    message: error_str.to_string(),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
        }
    }

    Ok(diagnostics)
}

/// Parse a single PHPStan message object into an LSP `Diagnostic`.
fn parse_phpstan_message(msg: &serde_json::Value) -> Option<Diagnostic> {
    let message = msg.get("message")?.as_str()?;
    // PHPStan lines are 1-based; LSP lines are 0-based.
    let line = msg.get("line").and_then(|l| l.as_u64()).unwrap_or(1);
    let lsp_line = line.saturating_sub(1) as u32;

    // PHPStan may include an identifier (e.g. "argument.type",
    // "return.type", "method.notFound") since PHPStan 1.11.
    let identifier = msg
        .get("identifier")
        .and_then(|i| i.as_str())
        .unwrap_or("phpstan");

    let tip = msg.get("tip").and_then(|t| t.as_str());

    let full_message = if let Some(tip_text) = tip {
        // Strip HTML tags that PHPStan sometimes includes in tips
        // (e.g. <fg=cyan>...</>).
        let clean_tip = strip_ansi_tags(tip_text);
        format!("{}\n{}", message, clean_tip)
    } else {
        message.to_string()
    };

    // PHPStan includes `"ignorable": false` for errors that cannot be
    // suppressed with `@phpstan-ignore` (e.g. visibility overrides).
    // Store this in `Diagnostic.data` so code actions can check it.
    // Default to `true` when the field is absent (older PHPStan versions).
    let ignorable = msg
        .get("ignorable")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let data = Some(serde_json::json!({ "ignorable": ignorable }));

    Some(Diagnostic {
        range: crate::process::full_line_range(lsp_line),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(identifier.to_string())),
        code_description: None,
        source: Some("phpstan".to_string()),
        message: full_message,
        related_information: None,
        tags: None,
        data,
    })
}

/// Strip Symfony Console ANSI-style tags like `<fg=cyan>` and `</>`.
fn strip_ansi_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Write content to a temporary file for PHPStan's `--tmp-file` flag.
///
/// The temp file is created as a hidden sibling of `original` (same
/// directory, leading-dot name) so that PHPStan analyses it under a
/// path inside the real file's directory. Path-dependent rules read
/// the analysed file's path during editor mode, so a temp file in the
/// system temp dir would make them misfire. The leading dot keeps
/// PHPStan's file finder from picking the temp file up as a separately
/// analysed file. When the project directory is not writable, we fall
/// back to the system temp dir. The `.php` suffix is preserved because
/// PHPStan requires it. Returns a `NamedTempFile` whose destructor
/// removes the file on drop — the caller must keep it alive until after
/// PHPStan finishes.
fn write_temp_file(original: &Path, content: &str) -> Result<NamedTempFile, String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(".phpantom-").suffix(".php");

    let mut temp = match original.parent() {
        Some(dir) => builder
            .tempfile_in(dir)
            .or_else(|_| builder.tempfile())
            .map_err(|e| format!("Failed to create temp file: {}", e))?,
        None => builder
            .tempfile()
            .map_err(|e| format!("Failed to create temp file: {}", e))?,
    };

    temp.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    temp.flush()
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;

    Ok(temp)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_phpstan_json_workspace ────────────────────────────────

    #[test]
    fn parse_workspace_json_groups_by_file() {
        let json = r#"{
            "totals": {"errors": 1, "file_errors": 3},
            "files": {
                "/proj/src/A.php": {"errors": 1, "messages": [
                    {"message": "Error A", "line": 5, "ignorable": true, "identifier": "argument.type"}
                ]},
                "src/B.php": {"errors": 1, "messages": [
                    {"message": "Error B", "line": 2}
                ]},
                "/proj/src/C.php (in context of class App\\C)": {"errors": 1, "messages": [
                    {"message": "Error C", "line": 9}
                ]}
            },
            "errors": ["config warning without a file"]
        }"#;

        let map = parse_phpstan_json_workspace(json, Path::new("/proj")).unwrap();
        assert_eq!(map.len(), 3);

        let a = &map[Path::new("/proj/src/A.php")];
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].message, "Error A");
        assert_eq!(a[0].range.start.line, 4);

        // Relative paths resolve against the workspace root.
        assert!(map.contains_key(Path::new("/proj/src/B.php")));
        // Trait-context suffixes are stripped from the key.
        assert!(map.contains_key(Path::new("/proj/src/C.php")));
    }

    #[test]
    fn parse_workspace_json_empty_files() {
        let json = r#"{"totals": {"errors": 0, "file_errors": 0}, "files": {}, "errors": []}"#;
        let map = parse_phpstan_json_workspace(json, Path::new("/proj")).unwrap();
        assert!(map.is_empty());
    }

    // ── strip_ansi_tags ─────────────────────────────────────────────

    #[test]
    fn strip_ansi_tags_no_tags() {
        assert_eq!(strip_ansi_tags("hello world"), "hello world");
    }

    #[test]
    fn strip_ansi_tags_with_symfony_tags() {
        assert_eq!(
            strip_ansi_tags("Use <fg=cyan>--level 5</> instead."),
            "Use --level 5 instead."
        );
    }

    #[test]
    fn strip_ansi_tags_multiple() {
        assert_eq!(
            strip_ansi_tags("<fg=red>error</>: <fg=cyan>hint</>"),
            "error: hint"
        );
    }

    // ── parse_phpstan_json ──────────────────────────────────────────

    #[test]
    fn parse_empty_result() {
        let json = r#"{"totals":{"errors":0,"file_errors":0},"files":{},"errors":[]}"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn parse_file_errors() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 2},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 2,
                    "messages": [
                        {
                            "message": "Parameter #1 $x of method Foo::bar() expects int, string given.",
                            "line": 10,
                            "ignorable": true,
                            "identifier": "argument.type"
                        },
                        {
                            "message": "Method Foo::baz() should return string but returns int.",
                            "line": 25,
                            "ignorable": true,
                            "identifier": "return.type"
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 2);

        // First diagnostic
        assert_eq!(diags[0].range.start.line, 9); // 10 - 1
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diags[0].source.as_deref(), Some("phpstan"));
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("argument.type".to_string()))
        );
        assert!(diags[0].message.contains("Parameter #1"));
        // ignorable: true is stored in data
        assert_eq!(
            diags[0].data,
            Some(serde_json::json!({ "ignorable": true }))
        );

        // Second diagnostic
        assert_eq!(diags[1].range.start.line, 24); // 25 - 1
        assert_eq!(
            diags[1].code,
            Some(NumberOrString::String("return.type".to_string()))
        );
        assert_eq!(
            diags[1].data,
            Some(serde_json::json!({ "ignorable": true }))
        );
    }

    #[test]
    fn parse_non_ignorable_diagnostic() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 1},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "messages": [
                        {
                            "message": "Private method Foo::bar() overriding public method Parent::bar() should also be public.",
                            "line": 10,
                            "ignorable": false,
                            "identifier": "method.visibility"
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].data,
            Some(serde_json::json!({ "ignorable": false }))
        );
    }

    #[test]
    fn parse_ignorable_defaults_true_when_field_absent() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 1},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "messages": [
                        {
                            "message": "Some error.",
                            "line": 1,
                            "identifier": "some.error"
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        // When "ignorable" is absent from JSON, default to true
        assert_eq!(
            diags[0].data,
            Some(serde_json::json!({ "ignorable": true }))
        );
    }

    #[test]
    fn parse_top_level_errors() {
        let json = r#"{
            "totals": {"errors": 1, "file_errors": 0},
            "files": {},
            "errors": ["PHPStan requires PHP >= 7.2.0, you have 7.1.0"]
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].range.start.line, 0);
        assert!(diags[0].message.contains("PHP >= 7.2.0"));
    }

    #[test]
    fn parse_message_with_tip() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 1},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "messages": [
                        {
                            "message": "Call to an undefined method Foo::bar().",
                            "line": 5,
                            "ignorable": true,
                            "identifier": "method.notFound",
                            "tip": "Use <fg=cyan>--level 5</> to see this."
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Call to an undefined method"));
        assert!(diags[0].message.contains("Use --level 5 to see this."));
        // Verify the ANSI tags were stripped
        assert!(!diags[0].message.contains("<fg=cyan>"));
    }

    #[test]
    fn parse_message_without_identifier() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 1},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "messages": [
                        {
                            "message": "Some old-style error.",
                            "line": 1,
                            "ignorable": true
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("phpstan".to_string()))
        );
    }

    #[test]
    fn parse_relative_path_match() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 1},
            "files": {
                "/home/user/project/src/Foo.php": {
                    "errors": 1,
                    "messages": [
                        {
                            "message": "Error in Foo.",
                            "line": 3,
                            "ignorable": true,
                            "identifier": "phpstan"
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        // Use a relative path that should still match via suffix.
        let path = Path::new("src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn parse_no_matching_file() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 1},
            "files": {
                "/project/src/Bar.php": {
                    "errors": 1,
                    "messages": [
                        {
                            "message": "Error in Bar.",
                            "line": 1,
                            "ignorable": true,
                            "identifier": "phpstan"
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn parse_invalid_json() {
        let result = parse_phpstan_json("not json", Path::new("Foo.php"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_message_line_zero_defaults_to_line_1() {
        let json = r#"{
            "totals": {"errors": 0, "file_errors": 1},
            "files": {
                "/project/src/Foo.php": {
                    "errors": 1,
                    "messages": [
                        {
                            "message": "Error without line.",
                            "ignorable": true,
                            "identifier": "phpstan"
                        }
                    ]
                }
            },
            "errors": []
        }"#;
        let path = Path::new("/project/src/Foo.php");
        let diags = parse_phpstan_json(json, path).unwrap();
        assert_eq!(diags.len(), 1);
        // Line defaults to 1, which becomes 0 in LSP (1 - 1 = 0).
        assert_eq!(diags[0].range.start.line, 0);
    }

    // ── resolve_phpstan ─────────────────────────────────────────────

    /// Helper: parse a JSON string into a [`crate::composer::ComposerPackage`]
    /// declaring a `require-dev` dependency on `phpstan/phpstan`.
    fn pkg_with_phpstan() -> crate::composer::ComposerPackage {
        r#"{"require-dev": {"phpstan/phpstan": "^2.1"}}"#.parse().unwrap()
    }

    /// Helper: a workspace with an executable `vendor/bin/phpstan` in it.
    fn workspace_with_vendor_phpstan() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin_path).unwrap();
        let phpstan = bin_path.join("phpstan");
        std::fs::write(&phpstan, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&phpstan, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        (dir, phpstan)
    }

    #[test]
    fn resolve_disabled_when_empty_string() {
        let config = PhpStanConfig {
            command: Some(String::new()),
            memory_limit: None,
            timeout: None,
        };
        let result = resolve_phpstan(None, &config, None, None);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_explicit_command() {
        let config = PhpStanConfig {
            command: Some("/usr/bin/phpstan".to_string()),
            memory_limit: None,
            timeout: None,
        };
        // Explicit command bypasses the composer.json dependency check.
        let result = resolve_phpstan(None, &config, None, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, PathBuf::from("/usr/bin/phpstan"));
    }

    #[test]
    fn resolve_auto_detect_vendor_bin() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin_path).unwrap();
        let phpstan = bin_path.join("phpstan");
        std::fs::write(&phpstan, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&phpstan, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = PhpStanConfig::default();
        let package = pkg_with_phpstan();
        let result = resolve_phpstan(Some(dir.path()), &config, None, Some(&package));
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, phpstan);
    }

    #[test]
    fn resolve_auto_detect_custom_bin_dir() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("tools");
        std::fs::create_dir_all(&bin_path).unwrap();
        let phpstan = bin_path.join("phpstan");
        std::fs::write(&phpstan, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&phpstan, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = PhpStanConfig::default();
        let package = pkg_with_phpstan();
        let result = resolve_phpstan(Some(dir.path()), &config, Some("tools"), Some(&package));
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, phpstan);
    }

    #[test]
    fn resolve_no_binary_found() {
        let dir = tempfile::tempdir().unwrap();
        let config = PhpStanConfig::default();
        let package = pkg_with_phpstan();
        // No vendor/bin/phpstan, and PATH is unlikely to have it in test env.
        // This test may still find phpstan on PATH in some environments,
        // so we just verify it doesn't panic.
        let _ = resolve_phpstan(Some(dir.path()), &config, None, Some(&package));
    }

    #[test]
    fn resolve_auto_detect_skipped_without_composer_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin_path).unwrap();
        let phpstan = bin_path.join("phpstan");
        std::fs::write(&phpstan, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&phpstan, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = PhpStanConfig::default();
        // A binary sits at vendor/bin/phpstan, but composer.json does not
        // depend on phpstan/phpstan (e.g. a stale binary, or one installed
        // for another project) — auto-detect must not pick it up from
        // vendor/bin. It may still find an unrelated `phpstan` on $PATH in
        // some environments, so only assert the vendor/bin one is excluded.
        let package: crate::composer::ComposerPackage = r#"{"require": {}}"#.parse().unwrap();
        let result = resolve_phpstan(Some(dir.path()), &config, None, Some(&package));
        assert_ne!(result.map(|r| r.path), Some(phpstan.clone()));

        // No composer.json at all behaves the same way.
        let result = resolve_phpstan(Some(dir.path()), &config, None, None);
        assert_ne!(result.map(|r| r.path), Some(phpstan));
    }

    /// A Laravel project may install any of the PHPStan extensions that
    /// understand the framework, or a fork of one, and each pulls
    /// `phpstan/phpstan` in transitively rather than declaring it — so each
    /// has to certify `vendor/bin/phpstan` on its own.
    #[test]
    fn resolve_auto_detect_laravel_extension_on_laravel_project() {
        for extension in [
            "larastan/larastan",
            "calebdw/larastan",
            "calebdw/phpstan-laravel",
            "acme/phpstan-laravel",
        ] {
            let (dir, phpstan) = workspace_with_vendor_phpstan();
            let package: crate::composer::ComposerPackage = format!(
                r#"{{"require": {{
                    "laravel/framework": "^11.0",
                    "{extension}": "*"
                }}}}"#
            )
            .parse()
            .unwrap();

            let result = resolve_phpstan(
                Some(dir.path()),
                &PhpStanConfig::default(),
                None,
                Some(&package),
            );
            assert_eq!(result.unwrap().path, phpstan, "for `{extension}`");
        }
    }

    #[test]
    fn resolve_auto_detect_skipped_on_laravel_project_without_laravel_extension() {
        let (dir, _phpstan) = workspace_with_vendor_phpstan();

        let config = PhpStanConfig::default();
        // A Laravel project that depends directly on phpstan/phpstan but
        // has installed no Laravel-aware extension must not auto-run plain
        // PHPStan: it does not understand Eloquent magic, facades, or
        // container bindings, and would report false positives against
        // them.  The refusal covers `$PATH` too, so nothing resolves at
        // all: a global binary would misread the framework the same way.
        let package: crate::composer::ComposerPackage = r#"{"require": {
            "laravel/framework": "^11.0",
            "phpstan/phpstan": "^2.1"
        }}"#
        .parse()
        .unwrap();
        let result = resolve_phpstan(Some(dir.path()), &config, None, Some(&package));
        assert!(result.is_none(), "got {result:?}");
    }

    /// A library that requires an Illuminate component is not a Laravel
    /// application: it has no framework for plain PHPStan to misread, so
    /// its own direct `phpstan/phpstan` dependency certifies the binary.
    #[test]
    fn resolve_auto_detect_on_a_library_using_an_illuminate_component() {
        let (dir, phpstan) = workspace_with_vendor_phpstan();
        let package: crate::composer::ComposerPackage = r#"{"require": {
            "illuminate/support": "^11.0"
        }, "require-dev": {
            "phpstan/phpstan": "^2.1"
        }}"#
        .parse()
        .unwrap();

        let result = resolve_phpstan(
            Some(dir.path()),
            &PhpStanConfig::default(),
            None,
            Some(&package),
        );
        assert_eq!(result.unwrap().path, phpstan);
    }

    /// PHPStan reads three config-file spellings, and each of them
    /// certifies the vendored binary the same way — the workspace-run gate
    /// and this one read the same list.
    #[test]
    fn resolve_auto_detect_via_every_config_file_spelling() {
        for name in CONFIG_FILES {
            let (dir, phpstan) = workspace_with_vendor_phpstan();
            std::fs::write(dir.path().join(name), "").unwrap();
            assert!(has_project_config(dir.path()), "for `{name}`");

            let result = resolve_phpstan(Some(dir.path()), &PhpStanConfig::default(), None, None);
            assert_eq!(result.unwrap().path, phpstan, "for `{name}`");
        }
    }

    #[test]
    fn resolve_auto_detect_via_phpstan_neon_without_composer_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin_path).unwrap();
        let phpstan = bin_path.join("phpstan");
        std::fs::write(&phpstan, "#!/bin/sh\n").unwrap();
        std::fs::write(dir.path().join("phpstan.neon"), "").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&phpstan, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = PhpStanConfig::default();
        // A hand-authored phpstan.neon certifies PHPStan use on its own,
        // even with no composer.json (or one that names neither package).
        let result = resolve_phpstan(Some(dir.path()), &config, None, None);
        assert_eq!(result.unwrap().path, phpstan);
    }

    #[test]
    fn resolve_auto_detect_via_phpstan_neon_dist_on_laravel_project_without_extension() {
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin_path).unwrap();
        let phpstan = bin_path.join("phpstan");
        std::fs::write(&phpstan, "#!/bin/sh\n").unwrap();
        std::fs::write(dir.path().join("phpstan.neon.dist"), "").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&phpstan, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = PhpStanConfig::default();
        // Same extensionless Laravel project as the test above, but this
        // one hand-authors a phpstan.neon.dist: the config file overrides
        // the extension gate rather than being blocked by it.
        let package: crate::composer::ComposerPackage = r#"{"require": {
            "laravel/framework": "^11.0",
            "phpstan/phpstan": "^2.1"
        }}"#
        .parse()
        .unwrap();
        let result = resolve_phpstan(Some(dir.path()), &config, None, Some(&package));
        assert_eq!(result.unwrap().path, phpstan);
    }

    // ── write_temp_file ─────────────────────────────────────────────

    #[test]
    fn write_temp_file_places_sibling_of_original() {
        let content = "<?php\necho 'hello';\n";
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("src");
        std::fs::create_dir_all(&original).unwrap();
        let original = original.join("Foo.php");

        let tmp = write_temp_file(&original, content).unwrap();

        assert!(tmp.path().exists());
        // The temp file is a hidden sibling in the original's directory.
        assert_eq!(tmp.path().parent(), original.parent());
        assert!(tmp.path().extension().and_then(|e| e.to_str()) == Some("php"));
        let name = tmp
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        assert!(name.starts_with(".phpantom-"));

        let read_back = std::fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(read_back, content);

        // NamedTempFile auto-deletes on drop — no manual remove needed.
    }

    #[test]
    fn write_temp_file_falls_back_when_dir_missing() {
        let content = "<?php\necho 'hi';\n";
        // Parent directory does not exist: creation there fails and we
        // fall back to the system temp dir rather than erroring.
        let original = Path::new("/nonexistent-phpantom-dir/src/Foo.php");
        let tmp = write_temp_file(original, content).unwrap();

        assert!(tmp.path().exists());
        assert!(tmp.path().extension().and_then(|e| e.to_str()) == Some("php"));
        let read_back = std::fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(read_back, content);
    }

    // ── PhpStanConfig helpers ───────────────────────────────────────

    #[test]
    fn config_timeout_default() {
        let config = PhpStanConfig::default();
        assert_eq!(config.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn config_timeout_custom() {
        let config = PhpStanConfig {
            command: None,
            memory_limit: None,
            timeout: Some(30_000),
        };
        assert_eq!(config.timeout_ms(), 30_000);
    }

    #[test]
    fn config_is_disabled() {
        let disabled = PhpStanConfig {
            command: Some(String::new()),
            memory_limit: None,
            timeout: None,
        };
        assert!(disabled.is_disabled());

        let enabled = PhpStanConfig::default();
        assert!(!enabled.is_disabled());

        let explicit = PhpStanConfig {
            command: Some("/usr/bin/phpstan".to_string()),
            memory_limit: None,
            timeout: None,
        };
        assert!(!explicit.is_disabled());
    }
}
