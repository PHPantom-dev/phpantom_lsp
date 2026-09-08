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
//!    `composer.json` lists `laravel/pint` or `friendsofphp/php-cs-fixer`
//!    in `require-dev`, resolve the binary via Composer's bin-dir and
//!    run it as a subprocess.  `squizlabs/php_codesniffer` does the same,
//!    or, if the project pulls it in only transitively (e.g. through
//!    `slevomat/coding-standard`), a `phpcs.xml`/`.phpcs.xml` config file
//!    at the workspace root certifies phpcbf just as well — except on a
//!    project whose `mago.toml` has a `[formatter]` table, which lints
//!    with PHPCS and formats with Mago.
//! 3. **Otherwise, use mago-formatter.**  No subprocess, no temp files,
//!    no external dependencies.  Uses PER-CS 2.0 defaults or if present `mago.toml`.
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

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::config::FormattingConfig;

mod external;
mod mago;
#[cfg(test)]
mod tests;

const DEFAULT_TIMEOUT_MS: u64 = 10_000;

// ── Tool resolution ─────────────────────────────────────────────────

/// A resolved formatting tool ready to invoke.
#[derive(Debug, Clone)]
pub struct ResolvedTool {
    /// Human-readable name for logging.
    pub name: &'static str,
    /// Absolute or relative path to the binary.
    pub path: PathBuf,
}

/// The resolved formatting strategy: external tools, built-in
/// formatter, or disabled.
#[derive(Debug)]
pub enum FormattingStrategy {
    /// Run one or more external tools in sequence.
    External(Vec<ResolvedTool>),
    /// Use the built-in mago-formatter with optional `mago.toml`
    BuiltIn(Option<PathBuf>),
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
/// - If `composer_json` has `laravel/pint`, `friendsofphp/php-cs-fixer`,
///   or `squizlabs/php_codesniffer` in `require-dev`, or the workspace
///   root has a `phpcs.xml`/`.phpcs.xml`/`phpcs.xml.dist`/
///   `.phpcs.xml.dist` config file (which certifies phpcbf even when
///   PHP_CodeSniffer is pulled in only transitively) → `External`,
///   resolving paths via the Composer bin-dir.  A `[formatter]` table in
///   the workspace `mago.toml` takes phpcbf back out of that set: it says
///   what the project formats with, where PHPCS says what it lints with.
/// - Otherwise → `BuiltIn`.
pub fn resolve_strategy(
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    composer_json: Option<&crate::composer::ComposerPackage>,
    bin_dir: Option<&str>,
) -> FormattingStrategy {
    if config.is_disabled() {
        return FormattingStrategy::Disabled;
    }

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

    // No explicit config — check composer.json require-dev, or a
    // hand-authored phpcs config file for phpcbf.
    let has_phpcs_config = workspace_root.is_some_and(crate::phpcs::has_project_config);
    let formats_with_mago = workspace_root.is_some_and(crate::mago::formats_with_mago);

    if composer_json.is_some() || has_phpcs_config {
        let mut tools = Vec::new();
        let bin = bin_dir.unwrap_or("vendor/bin");

        // Only one of the config values can be Some("") here (disabling
        // one tool while leaving the other to auto-detect).
        let fixer_disabled = config.php_cs_fixer.as_deref() == Some("");
        let phpcbf_disabled = config.phpcbf.as_deref() == Some("");
        let pint_disabled = config.pint.as_deref() == Some("");

        if !pint_disabled
            && composer_json
                .is_some_and(|package| crate::composer::has_require_dev(package, "laravel/pint"))
            && let Some(tool) = resolve_from_bin_dir("pint", workspace_root, bin)
        {
            tools.push(tool);
        }

        if !fixer_disabled
            && composer_json.is_some_and(|package| {
                crate::composer::has_require_dev(package, "friendsofphp/php-cs-fixer")
            })
            && let Some(tool) = resolve_from_bin_dir("php-cs-fixer", workspace_root, bin)
        {
            tools.push(tool);
        }

        // A phpcs config file certifies phpcbf on its own: a project
        // that pulls squizlabs/php_codesniffer in only transitively
        // (e.g. through slevomat/coding-standard) never lists it in
        // require-dev directly, but a phpcs.xml is still deliberate
        // evidence the project uses it.
        //
        // Unless the project also says what it formats with.  PHP_CodeSniffer
        // is a linter that happens to ship a fixer, so its ruleset is
        // evidence of linting first; a `[formatter]` table in `mago.toml` is
        // evidence of nothing else.  A project carrying both lints with
        // PHPCS and formats with Mago.
        if !phpcbf_disabled
            && !formats_with_mago
            && (has_phpcs_config
                || composer_json.is_some_and(|package| {
                    crate::composer::has_require_dev(package, "squizlabs/php_codesniffer")
                }))
            && let Some(tool) = resolve_from_bin_dir("phpcbf", workspace_root, bin)
        {
            tools.push(tool);
        }

        if !tools.is_empty() {
            return FormattingStrategy::External(tools);
        }
    }

    // No external tools configured or detected — use built-in.
    let config_path = workspace_root
        .filter(|root| crate::mago::has_mago_config(root))
        .map(|root| root.join("mago.toml"));
    FormattingStrategy::BuiltIn(config_path)
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

// ── Execution ───────────────────────────────────────────────────────

/// Execute the resolved formatting strategy and return `TextEdit`s.
///
/// This is the main entry point called by the `formatting()` handler.
/// `cancelled` aborts any running external tool when set (the server's
/// shutdown flag).
pub fn execute_strategy(
    strategy: &FormattingStrategy,
    content: &str,
    file_path: &Path,
    config: &FormattingConfig,
    php_version: crate::types::PhpVersion,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<TextEdit>>, String> {
    match strategy {
        FormattingStrategy::Disabled => Ok(None),
        FormattingStrategy::External(tools) => {
            let edits =
                external::run_external_pipeline(tools, content, file_path, config, cancelled)?;
            if edits.is_empty() {
                Ok(None)
            } else {
                Ok(Some(edits))
            }
        }
        FormattingStrategy::BuiltIn(config_path) => {
            let mago_version = mago::to_mago_php_version(php_version);
            let settings = match config_path {
                Some(config_path) => mago::load_mago_format_settings(config_path)?,
                None => mago_formatter::settings::FormatSettings::default(),
            };
            let formatted = mago::format_with_mago(content, mago_version, settings)?;
            let edits = compute_edits(content, &formatted);
            if edits.is_empty() {
                Ok(None)
            } else {
                Ok(Some(edits))
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
