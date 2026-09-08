//! CLI analysis mode.
//!
//! Scans PHP files in a project and reports PHPantom's own diagnostics
//! (no PHPStan, no external tools) in a PHPStan-like table format.
//!
//! # Philosophy
//!
//! The goal is **100% type coverage**: every class, member, and function
//! call in the project should be resolvable by the LSP.  When that holds,
//! completion works everywhere with no dead spots, and downstream tools
//! like PHPStan get the type information they need to find real bugs at
//! every level.  PHPStan only complains about missing types at levels 6,
//! 9, and 10; PHPantom fills those gaps cheaply and immediately so
//! PHPStan can focus on logic errors rather than fighting incomplete
//! type information.
//!
//! The diagnostics reported here are not trying to be a static analyser.
//! They assert structural correctness: does this class exist, does this
//! member exist, does the argument count match, did you implement every
//! required method.  Bug hunting is left to dedicated tools like PHPStan
//! and Psalm.  The `analyze` command surfaces the places where the LSP
//! cannot resolve a symbol so the user can fix them and achieve (or
//! maintain) full completion coverage across the project.
//!
//! It reuses the same `Backend` initialization pipeline as the LSP
//! server, so the results match exactly what a user would see in their
//! editor.
//!
//! Composer projects (root `composer.json`) use their autoload
//! configuration for file discovery; a plain PHP tree without one is
//! analysed by scanning and walking the workspace root.  Multi-project
//! monorepos are not supported.
//!
//! # Usage
//!
//! ```sh
//! phpantom_lsp analyze                     # scan entire project
//! phpantom_lsp analyze src/                # scan a subdirectory
//! phpantom_lsp analyze src/Foo.php         # scan a single file
//! ```
//!
//! The driver and file discovery live in [`run`]; output formatting
//! (table, GitHub annotations, JSON) lives in [`output`]; opening the
//! project on a headless `Backend`, which `fix` and `move` share, lives
//! in [`project`].

use std::path::PathBuf;

use tower_lsp::lsp_types::DiagnosticSeverity;

mod output;
mod project;
mod run;

pub(crate) use output::{format_github_message, json_escape, print_success_box, progress_bar};
pub(crate) use project::{
    discover_laravel_resources, load_config_or_default, open_headless_project, parse_user_files,
};
pub(crate) use run::discover_user_files;
pub use run::run;

/// Severity filter for the analyse output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeverityFilter {
    /// Show all diagnostics (error, warning, information, hint).
    All,
    /// Show only errors and warnings.
    Warning,
    /// Show only errors.
    Error,
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable PHPStan-style table (default).
    Table,
    /// GitHub Actions workflow commands (`::error file=...::message`).
    /// Diagnostics appear as inline annotations on pull request diffs.
    Github,
    /// JSON object with totals and per-file diagnostics.
    Json,
}

/// Options for the analyse command.
#[derive(Debug)]
pub struct AnalyseOptions {
    /// Workspace root.  Usually a Composer project directory; a plain
    /// PHP tree without a composer.json is analysed by walking the
    /// root.
    pub workspace_root: PathBuf,
    /// Optional path filters: only analyse files under these paths.
    /// Each can be a directory or a single file; empty means the whole
    /// project.
    pub path_filters: Vec<PathBuf>,
    /// Minimum severity to report.
    pub severity_filter: SeverityFilter,
    /// Whether to output with ANSI colours.
    pub use_colour: bool,
    /// Output format.
    pub output_format: OutputFormat,
    /// Print each file path as it is processed (and disable the
    /// progress bar) so a hang is attributable to a specific file.
    pub debug: bool,
    /// Verbosity level: 0 = normal, 1 = -v, 2 = -vv, 3+ = -vvv.
    pub verbosity: u8,
    /// The global `.phpantom.toml` to merge underneath the project's own,
    /// or `None` to analyse against the project config alone.
    ///
    /// The CLI passes [`crate::config::global_config_path`] so a command-line
    /// run honours the same defaults the editor does. Tests leave it
    /// `None` so the machine's config directory cannot change what they
    /// assert.
    pub global_config: Option<PathBuf>,
}

/// A single diagnostic result for the analyse output.
struct FileDiagnostic {
    /// 1-based line number.
    line: u32,
    /// 0-based column, used only to order same-line diagnostics
    /// deterministically.
    column: u32,
    /// The diagnostic message.
    message: String,
    /// The diagnostic code (e.g. "unknown_class").
    identifier: Option<String>,
    /// The diagnostic severity.
    severity: DiagnosticSeverity,
}
