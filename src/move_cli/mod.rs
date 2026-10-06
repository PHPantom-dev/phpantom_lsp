//! Headless class and namespace moves for the command-line interface.
//!
//! The driver (resolving what the user named, planning the edits, and
//! applying them) lives in [`run`]; output formatting (table, GitHub
//! annotations, JSON) lives in [`output`]; the scan for mentions of the
//! old name that no rewrite could reach lives in [`residual`].

use std::path::PathBuf;

use crate::analyse::OutputFormat;

mod output;
mod residual;
mod run;

pub use run::{execute, run};

/// Options for the move command.
#[derive(Debug)]
pub struct MoveOptions {
    /// Source class, namespace, file, or directory.
    pub from: String,
    /// Destination class, namespace, file, or directory.
    pub to: String,
    /// Workspace root.
    pub workspace_root: PathBuf,
    /// Preview the move without writing files.
    pub dry_run: bool,
    /// Whether to output with ANSI colours.
    pub use_colour: bool,
    /// Output format, shared with `analyze` and `fix`.
    pub output_format: OutputFormat,
    /// Global configuration path.
    pub global_config: Option<PathBuf>,
}

/// What a move did, once it is too late to refuse it.
#[derive(Debug)]
pub struct MoveSummary {
    /// Whether the plan was reported rather than applied.
    pub dry_run: bool,
    /// `"class"` or `"namespace"`.
    pub kind: &'static str,
    /// The name the move started from.
    pub from: String,
    /// The name it arrived at.
    pub to: String,
    /// How many files were rewritten.
    pub files_changed: usize,
    /// How many files were renamed on disk.
    pub paths_moved: usize,
    /// Conditions the caller needs to act on even though the move applied.
    pub warnings: Vec<MoveWarning>,
}

/// Something the move left for the caller to deal with: a destination
/// the autoloader cannot reach, or a mention of the old name or path
/// the rewriter had no way to see.
#[derive(Debug)]
pub struct MoveWarning {
    /// What went wrong, without the location, which is carried
    /// separately so the JSON and GitHub formats can place it.
    pub message: String,
    /// The project-relative file the warning is about, after the move.
    pub file: Option<String>,
    /// The 1-based line within that file.
    pub line: Option<usize>,
}

struct MovePlan {
    writes: Vec<(PathBuf, String)>,
    moves: Vec<(PathBuf, PathBuf)>,
}
