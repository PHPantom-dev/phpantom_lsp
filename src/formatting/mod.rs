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
//! same config discovery without temp files, and reads `pint.json`
//! from its working directory, so every tool runs with the workspace
//! root as its working directory.
//!
//! ## Blade templates
//!
//! A `.blade.php` file is resolved separately, see [`blade`]: Pint when
//! the project has its Blade rule on, the built-in reindenter otherwise.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::Backend;
use crate::composer::{self, ComposerPackage};
use crate::config::FormattingConfig;

pub mod blade;
mod external;
mod mago;
mod pint;
#[cfg(test)]
mod tests;

const DEFAULT_TIMEOUT_MS: u64 = 10_000;

// ── Tool resolution ─────────────────────────────────────────────────

/// An external formatter PHPantom knows how to detect and drive.
///
/// Everything tool-specific hangs off this enum: the `.phpantom.toml`
/// key, the Composer package that certifies it, the binary name, and how
/// it is invoked.  Adding a formatter means adding a variant and filling
/// in each method; the resolution and execution code never lists tools
/// by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pint,
    PhpCsFixer,
    Phpcbf,
}

impl Tool {
    /// Every tool, in the order a pipeline runs them.
    pub const ALL: [Tool; 3] = [Tool::Pint, Tool::PhpCsFixer, Tool::Phpcbf];

    /// The binary's name, which is also the name used in logs.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Pint => "pint",
            Tool::PhpCsFixer => "php-cs-fixer",
            Tool::Phpcbf => "phpcbf",
        }
    }

    /// The Composer package whose presence in `require-dev` selects the
    /// tool.
    fn composer_package(self) -> &'static str {
        match self {
            Tool::Pint => "laravel/pint",
            Tool::PhpCsFixer => "friendsofphp/php-cs-fixer",
            Tool::Phpcbf => "squizlabs/php_codesniffer",
        }
    }

    /// The tool's `.phpantom.toml` entry: `None` when unset, `Some("")`
    /// when disabled, otherwise the command to run.
    pub fn configured(self, config: &FormattingConfig) -> Option<&str> {
        match self {
            Tool::Pint => config.pint.as_deref(),
            Tool::PhpCsFixer => config.php_cs_fixer.as_deref(),
            Tool::Phpcbf => config.phpcbf.as_deref(),
        }
    }

    /// Whether the project's own metadata says it formats with this tool.
    fn detected(
        self,
        workspace_root: Option<&Path>,
        composer_json: Option<&ComposerPackage>,
    ) -> bool {
        let in_require_dev = composer_json
            .is_some_and(|package| composer::has_require_dev(package, self.composer_package()));
        match self {
            // A phpcs config file certifies phpcbf on its own: a project
            // that pulls squizlabs/php_codesniffer in only transitively
            // (e.g. through slevomat/coding-standard) never lists it in
            // require-dev directly, but a phpcs.xml is still deliberate
            // evidence the project uses it.
            //
            // Unless the project also says what it formats with.
            // PHP_CodeSniffer is a linter that happens to ship a fixer, so
            // its ruleset is evidence of linting first; a `[formatter]`
            // table in `mago.toml` is evidence of nothing else.  A project
            // carrying both lints with PHPCS and formats with Mago.
            Tool::Phpcbf => {
                (in_require_dev || workspace_root.is_some_and(crate::phpcs::has_project_config))
                    && !workspace_root.is_some_and(crate::mago::formats_with_mago)
            }
            Tool::Pint | Tool::PhpCsFixer => in_require_dev,
        }
    }
}

/// A resolved formatting tool ready to invoke.
#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub tool: Tool,
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
/// - If `config.is_disabled()` (every tool set to `""`) → `Disabled`.
/// - If any tool has an explicit non-empty path in config →
///   `External` with those tools.
/// - Otherwise every tool the project's metadata selects (see
///   [`Tool::detected`]) and whose binary the Composer bin-dir holds →
///   `External`.
/// - Otherwise → `BuiltIn`.
pub fn resolve_strategy(
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    composer_json: Option<&ComposerPackage>,
    bin_dir: Option<&str>,
) -> FormattingStrategy {
    if config.is_disabled() {
        return FormattingStrategy::Disabled;
    }

    // Explicit config wins, and skips detection entirely.
    let explicit: Vec<ResolvedTool> = Tool::ALL
        .into_iter()
        .filter_map(|tool| {
            let command = tool.configured(config)?;
            (!command.is_empty()).then(|| ResolvedTool {
                tool,
                path: PathBuf::from(command),
            })
        })
        .collect();
    if !explicit.is_empty() {
        return FormattingStrategy::External(explicit);
    }

    // A tool set to `""` is disabled even when the project would select it.
    let bin = bin_dir.unwrap_or("vendor/bin");
    let detected: Vec<ResolvedTool> = Tool::ALL
        .into_iter()
        .filter(|tool| tool.configured(config) != Some(""))
        .filter(|tool| tool.detected(workspace_root, composer_json))
        .filter_map(|tool| resolve_from_bin_dir(tool, workspace_root, bin))
        .collect();
    if !detected.is_empty() {
        return FormattingStrategy::External(detected);
    }

    let config_path = workspace_root
        .filter(|root| crate::mago::has_mago_config(root))
        .map(|root| root.join("mago.toml"));
    FormattingStrategy::BuiltIn(config_path)
}

/// Resolve a tool binary from the Composer bin directory.
fn resolve_from_bin_dir(
    tool: Tool,
    workspace_root: Option<&Path>,
    bin_dir: &str,
) -> Option<ResolvedTool> {
    let candidate = workspace_root?.join(bin_dir).join(tool.name());
    candidate.is_file().then_some(ResolvedTool {
        tool,
        path: candidate,
    })
}

impl Backend {
    /// Resolve the workspace's formatting strategy from `.phpantom.toml`,
    /// the root `composer.json`, and the workspace root.
    ///
    /// Reads `composer.json` from disk, so call it once per request (or
    /// once per run), not per file.
    pub(crate) fn resolve_formatting_strategy(&self) -> FormattingStrategy {
        let config = self.config();
        let workspace_root = self.workspace.workspace_root.read().clone();
        let composer_json = workspace_root
            .as_deref()
            .and_then(composer::read_composer_package);
        let bin_dir = composer_json.as_ref().map(composer::get_bin_dir);
        resolve_strategy(
            workspace_root.as_deref(),
            &config.formatting,
            composer_json.as_ref(),
            bin_dir.as_deref(),
        )
    }

    /// Format one file's `content` with `strategy`.
    ///
    /// `file_path` is the file's real location, which external tools use
    /// to discover their project config.  Returns the formatted text, or
    /// `None` when formatting is disabled or the content is already
    /// formatted.  `cancelled` aborts a running external tool when set.
    pub(crate) fn format_content(
        &self,
        strategy: &FormattingStrategy,
        file_path: &Path,
        content: &str,
        cancelled: &AtomicBool,
    ) -> Result<Option<String>, String> {
        let config = self.config();
        let workspace_root = self.workspace.workspace_root.read().clone();
        format_content(
            strategy,
            content,
            file_path,
            workspace_root.as_deref(),
            &config.formatting,
            self.php_version(),
            cancelled,
        )
    }
}

// ── Execution ───────────────────────────────────────────────────────

/// Run `strategy` on `content` and return the formatted text, or `None`
/// when formatting is disabled or nothing changed.
///
/// External tools run with `workspace_root` as their working directory,
/// which is where Pint looks for `pint.json`.
pub fn format_content(
    strategy: &FormattingStrategy,
    content: &str,
    file_path: &Path,
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    php_version: crate::types::PhpVersion,
    cancelled: &AtomicBool,
) -> Result<Option<String>, String> {
    let formatted = match strategy {
        FormattingStrategy::Disabled => return Ok(None),
        FormattingStrategy::External(tools) => external::run_external_pipeline(
            tools,
            content,
            file_path,
            workspace_root,
            config,
            cancelled,
        )?,
        FormattingStrategy::BuiltIn(config_path) => {
            let mago_version = mago::to_mago_php_version(php_version);
            let settings = match config_path {
                Some(config_path) => mago::load_mago_format_settings(config_path)?,
                None => mago_formatter::settings::FormatSettings::default(),
            };
            mago::format_with_mago(content, mago_version, settings)?
        }
    };
    Ok((formatted != content).then_some(formatted))
}

/// Run `strategy` on `content` and return the `TextEdit`s that turn it
/// into the formatted text, or `None` when there is nothing to change.
pub fn execute_strategy(
    strategy: &FormattingStrategy,
    content: &str,
    file_path: &Path,
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    php_version: crate::types::PhpVersion,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<TextEdit>>, String> {
    let formatted = format_content(
        strategy,
        content,
        file_path,
        workspace_root,
        config,
        php_version,
        cancelled,
    )?;
    Ok(formatted.map(|formatted| compute_edits(content, &formatted)))
}

/// Compute the `TextEdit`s needed to transform `original` into `formatted`.
///
/// Returns a single `TextEdit` that replaces the entire document.  Only
/// returns edits if the content actually changed.
pub(crate) fn compute_edits(original: &str, formatted: &str) -> Vec<TextEdit> {
    if original == formatted {
        return Vec::new();
    }

    let line_count = original.lines().count();
    let last_line_idx = if line_count == 0 { 0 } else { line_count - 1 };
    // LSP columns are UTF-16 code units, not bytes.
    let last_line_len = original
        .lines()
        .last()
        .map_or(0, |l| l.encode_utf16().count());

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
