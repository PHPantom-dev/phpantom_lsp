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
//! The driver and file discovery live in [`run`], the stages it runs
//! through in [`stages`] and the parallel diagnostic pass in
//! [`diagnose`]; output formatting (table, GitHub annotations, JSON)
//! lives in [`output`]; opening the project on a headless `Backend`,
//! which `fix`, `move`, and `format` share, lives in [`project`].

use std::path::PathBuf;

use tower_lsp_server::gen_lsp_types::DiagnosticSeverity;

mod diagnose;
mod output;
mod project;
mod run;
mod stages;

pub(crate) use output::{
    Colour, JsonFileEntry, JsonMessage, JsonTotals, TableRow, dispatch_report,
    format_github_message, github_annotation, print_box, print_success_box, print_table,
    progress_bar,
};
pub(crate) use project::{
    discover_laravel_resources, load_config_or_default, open_headless_project,
    open_headless_project_unindexed, parse_user_files,
};
pub(crate) use run::discover_user_files;
pub use run::run;
pub(crate) use stages::{
    OpenedProject, note_plain_php_project, note_workspace_layout, open_project,
};

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

/// Which diagnostic codes the analyse output keeps.
///
/// The default keeps every diagnostic.
#[derive(Debug, Default)]
pub struct CodeFilter {
    only: Vec<String>,
    excluded: Vec<String>,
}

impl CodeFilter {
    /// Keep the codes in `only` (every code when it is empty), minus the
    /// codes in `excluded`.
    /// An old spelling of a renamed code is read as its current one.
    pub fn new(only: Vec<String>, excluded: Vec<String>) -> Self {
        let current = |codes: Vec<String>, flag: &str| {
            codes
                .iter()
                .map(|code| crate::diagnostics::identifiers::current_identifier_or_warn(code, flag))
                .collect()
        };
        Self {
            only: current(only, "--code"),
            excluded: current(excluded, "--exclude-code"),
        }
    }

    /// Whether a diagnostic with `code` is kept. A diagnostic without a code
    /// matches no name, so it is only kept when `only` is empty.
    fn passes(&self, code: Option<&str>) -> bool {
        let names = |list: &[String]| code.is_some_and(|code| list.iter().any(|c| c == code));
        (self.only.is_empty() || names(&self.only)) && !names(&self.excluded)
    }

    /// The codes `only` asks for that none of the `reported` codes match.
    ///
    /// A misspelled code filters out everything and reads as a clean run, so
    /// these are worth pointing out. A misspelled exclusion needs no such
    /// note: what it failed to exclude is still in the report.
    fn unmatched<'a>(&'a self, reported: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
        let mut unmatched: Vec<&str> = self.only.iter().map(String::as_str).collect();
        for code in reported {
            unmatched.retain(|wanted| *wanted != code);
        }
        unmatched
    }
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
    /// Which diagnostic codes to report.
    pub code_filter: CodeFilter,
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

#[cfg(test)]
mod tests {
    use super::CodeFilter;

    fn filter(only: &[&str], excluded: &[&str]) -> CodeFilter {
        let owned = |codes: &[&str]| codes.iter().map(ToString::to_string).collect();
        CodeFilter::new(owned(only), owned(excluded))
    }

    #[test]
    fn the_default_keeps_everything() {
        let filter = CodeFilter::default();
        assert!(filter.passes(Some("unused_import")));
        assert!(filter.passes(None));
    }

    #[test]
    fn only_keeps_the_named_codes() {
        let filter = filter(&["unused_import", "deprecated_usage"], &[]);
        assert!(filter.passes(Some("unused_import")));
        assert!(filter.passes(Some("deprecated_usage")));
        assert!(!filter.passes(Some("unknown_class")));
    }

    #[test]
    fn a_diagnostic_without_a_code_only_survives_when_nothing_is_asked_for_by_name() {
        assert!(!filter(&["unused_import"], &[]).passes(None));
        assert!(filter(&[], &["unused_import"]).passes(None));
    }

    #[test]
    fn excluded_wins_over_only() {
        let filter = filter(&["unused_import"], &["unused_import"]);
        assert!(!filter.passes(Some("unused_import")));
    }

    #[test]
    fn codes_match_exactly() {
        let filter = filter(&["unused_import"], &[]);
        assert!(!filter.passes(Some("unused")));
        assert!(!filter.passes(Some("UNUSED_IMPORT")));
    }

    #[test]
    fn unmatched_names_the_requested_codes_nothing_reported() {
        let filter = filter(
            &["unused_import", "unused_imprt", "deprecated_usage"],
            &["x"],
        );
        let reported = ["deprecated_usage", "unused_import", "deprecated_usage"];
        assert_eq!(filter.unmatched(reported.into_iter()), vec!["unused_imprt"]);
    }

    #[test]
    fn an_old_spelling_matches_the_renamed_code() {
        let only = filter(&["invalid_laravel_trans"], &[]);
        assert!(only.passes(Some("invalid_laravel_translation")));
        assert!(
            only.unmatched(["invalid_laravel_translation"].into_iter())
                .is_empty()
        );
        assert!(!filter(&[], &["invalid_member_access"]).passes(Some("inaccessible_member")));
    }

    #[test]
    fn unmatched_ignores_exclusions() {
        let filter = filter(&[], &["unused_imprt"]);
        assert!(filter.unmatched(std::iter::empty()).is_empty());
    }
}
