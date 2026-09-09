//! CLI formatting mode.
//!
//! Runs the same formatter the editor runs on `textDocument/formatting`
//! across a whole project, either writing the result back or, with
//! `--check`, reporting which files are not formatted and exiting
//! non-zero. That is the role `blade-formatter -c`, `phpcs`, and
//! `php-cs-fixer --dry-run` play in a CI pipeline, so a project can
//! enforce PHPantom's formatting on a pull request.
//!
//! # Usage
//!
//! ```sh
//! phpantom_lsp format                  # format the whole project in place
//! phpantom_lsp format --check          # report unformatted files, write nothing
//! phpantom_lsp format resources/views  # restrict to a subdirectory
//! phpantom_lsp format app/Foo.php      # format a single file
//! ```
//!
//! # Design
//!
//! Each file goes through the strategy [`crate::formatting`] resolves for
//! the project, so a run honours a detected Pint, php-cs-fixer, or phpcbf
//! exactly as the editor does, and falls back to the built-in
//! mago-formatter otherwise. A `.blade.php` template resolves separately
//! (see [`crate::formatting::blade`]): Pint when the project formats Blade
//! with it, the built-in reindenter otherwise.
//!
//! The run never builds a class index. Formatting asks nothing of it, so
//! the project is opened for its shape alone (see
//! [`crate::analyse::open_headless_project_unindexed`]) and the cost of a
//! run is the formatter itself.
//!
//! # Exit codes
//!
//! - `0` — every file is formatted, or every file was rewritten.
//! - `1` — a file could not be read, formatted, or written.
//! - `2` — `--check` found files that are not formatted.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::analyse::{OutputFormat, print_success_box, progress_bar};
use crate::config::FormattingConfig;
use crate::formatting::blade::{BladeFormatOptions, BladeFormattingStrategy};
use crate::formatting::{FormattingStrategy, Tool};
use crate::types::PhpVersion;
use crate::{Backend, composer, formatting};

/// Options for the format command.
#[derive(Debug)]
pub struct FormatOptions {
    /// Workspace root.  Usually a Composer project directory; a plain
    /// PHP tree without a composer.json is formatted by walking the root.
    pub workspace_root: PathBuf,
    /// Optional path filters: only format files under these paths.  Each
    /// can be a directory or a single file; empty means the whole project.
    pub path_filters: Vec<PathBuf>,
    /// Report the files that are not formatted and write nothing.
    pub check: bool,
    /// One level of indentation for the built-in Blade reindenter, which
    /// takes it from the editor over LSP and has no other source for it
    /// here.  The PHP formatter and the external tools read their own
    /// project config instead and ignore this.
    pub indent: String,
    /// Whether to output with ANSI colours.
    pub use_colour: bool,
    /// Output format, shared with `analyze`, `fix`, and `move`.
    pub output_format: OutputFormat,
    /// The global `.phpantom.toml` to merge underneath the project's own,
    /// or `None` to format against the project config alone.
    ///
    /// The CLI passes [`crate::config::global_config_path`] so a
    /// command-line run honours the same defaults the editor does. Tests
    /// leave it `None` so the machine's config directory cannot change
    /// what they assert.
    pub global_config: Option<PathBuf>,
}

/// A file whose formatted text differs from what is on disk.
struct Reformatted {
    /// Path relative to the workspace root, as reported.
    display_path: String,
    /// Absolute path to write back to.
    abs_path: PathBuf,
    /// The formatted text, held for the write phase.  `None` under
    /// `--check`, which reports the path and drops the text rather than
    /// carrying a second copy of every changed file in the project.
    formatted: Option<String>,
}

/// A file the run could not format.
struct Failure {
    display_path: String,
    message: String,
}

/// The outcome of formatting one file.
enum Outcome {
    /// Already formatted, or exempt from formatting.
    Unchanged,
    Reformatted(Reformatted),
    Failed(Failure),
}

/// What a run found, before it is reported.
struct Outcomes {
    /// Files whose formatted text differs from disk, sorted by path.
    reformatted: Vec<Reformatted>,
    /// Files the run could not format, sorted by path.
    failures: Vec<Failure>,
    /// How many files the run considered, for the summary line.
    file_count: usize,
}

/// Run the format command and return the process exit code.
pub fn run(options: FormatOptions) -> i32 {
    let Some(outcomes) = collect(&options) else {
        return 0;
    };

    report(&outcomes, &options);

    if !outcomes.failures.is_empty() {
        return 1;
    }
    if options.check && !outcomes.reformatted.is_empty() {
        return 2;
    }
    0
}

/// Format the project and, unless `options.check` is set, write every
/// changed file back.
///
/// `None` when the run had nothing to do at all: formatting is disabled,
/// or no file matched.  Both are reported on stderr where they happen,
/// because neither says anything about whether the project is formatted.
fn collect(options: &FormatOptions) -> Option<Outcomes> {
    let root = &options.workspace_root;

    // A missing composer.json is not an error: a plain PHP tree formats
    // fine with the built-in formatter.  Note it on stderr so a mistyped
    // --project-root does not silently rewrite the wrong directory.
    if !root.join("composer.json").is_file() {
        eprintln!(
            "Note: no composer.json found in {} - treating it as a plain PHP project.",
            root.display()
        );
    }

    // ── 1. Open the project (config and source layout, no index) ────
    let cfg = crate::analyse::load_config_or_default(root, options.global_config.as_deref());
    let formatting_config = cfg.formatting.clone();
    let backend = Backend::new_headless();
    crate::analyse::open_headless_project_unindexed(&backend, root, cfg);
    let php_version = backend.php_version();

    // ── 2. Resolve the strategies once for the whole run ────────────
    // Both resolvers read the same composer.json, so it is read here and
    // handed to each of them rather than once per resolver.
    let composer_json = composer::read_composer_package(root);
    let bin_dir = composer_json.as_ref().map(composer::get_bin_dir);
    let strategy = formatting::resolve_strategy(
        Some(root),
        &formatting_config,
        composer_json.as_ref(),
        bin_dir.as_deref(),
    );
    let blade_strategy = formatting::blade::resolve_blade_strategy(
        Some(root),
        &formatting_config,
        composer_json.as_ref(),
        bin_dir.as_deref(),
    );

    // Formatting off in `.phpantom.toml` means there is nothing to
    // enforce.  Say so rather than reporting every file as formatted,
    // which would read as a passing check.
    if matches!(strategy, FormattingStrategy::Disabled)
        && matches!(blade_strategy, BladeFormattingStrategy::Disabled)
    {
        eprintln!("Note: formatting is disabled in .phpantom.toml, nothing to do.");
        return None;
    }

    // ── 3. Discover files ───────────────────────────────────────────
    let files = crate::analyse::discover_user_files(&backend, root, &options.path_filters);
    if files.is_empty() {
        eprintln!("No PHP files found.");
        return None;
    }

    eprintln!(
        "Formatting {} {} with {} ({} for Blade templates).",
        files.len(),
        plural(files.len()),
        describe_strategy(&strategy),
        describe_blade_strategy(&blade_strategy),
    );

    // ── 4. Format every file (parallel) ─────────────────────────────
    let blade_options = BladeFormatOptions {
        indent: options.indent.clone(),
        ..BladeFormatOptions::default()
    };
    let context = Context {
        root,
        strategy: &strategy,
        blade_strategy: &blade_strategy,
        config: &formatting_config,
        blade_options: &blade_options,
        php_version,
        keep_text: !options.check,
        cancelled: AtomicBool::new(false),
    };
    let show_progress = options.use_colour && options.output_format == OutputFormat::Table;
    let (mut reformatted, mut failures) = format_files(&context, &files, show_progress);
    reformatted.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    failures.sort_by(|a, b| a.display_path.cmp(&b.display_path));

    // ── 5. Write back what the check would only have reported ───────
    for file in &reformatted {
        let Some(formatted) = &file.formatted else {
            continue;
        };
        if let Err(e) = std::fs::write(&file.abs_path, formatted) {
            failures.push(Failure {
                display_path: file.display_path.clone(),
                message: format!("failed to write: {e}"),
            });
        }
    }

    Some(Outcomes {
        reformatted,
        failures,
        file_count: files.len(),
    })
}

/// Everything formatting one file needs, hoisted out of the per-file loop
/// so the strategies, the config, and the PHP version are resolved once
/// for the run rather than once per file.
struct Context<'a> {
    root: &'a Path,
    strategy: &'a FormattingStrategy,
    blade_strategy: &'a BladeFormattingStrategy,
    config: &'a FormattingConfig,
    blade_options: &'a BladeFormatOptions,
    php_version: PhpVersion,
    /// Whether the formatted text is worth keeping; see
    /// [`Reformatted::formatted`].
    keep_text: bool,
    cancelled: AtomicBool,
}

/// Format every file in `files` on parallel workers and return the ones
/// whose formatted text differs from disk, plus the ones that failed.
///
/// The workers get [`crate::PARSE_WORKER_STACK_SIZE`]: the built-in
/// formatter is a recursive-descent parser and printer, and overflows the
/// 2 MB default a spawned thread otherwise has.
fn format_files(
    context: &Context<'_>,
    files: &[PathBuf],
    show_progress: bool,
) -> (Vec<Reformatted>, Vec<Failure>) {
    let file_count = files.len();
    // Every worker reserves PARSE_WORKER_STACK_SIZE, so a run over a
    // handful of files must not spawn one per core.
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(file_count);
    let next_idx = AtomicUsize::new(0);

    if show_progress {
        eprint!("\r\x1b[2K {}", progress_bar(0, file_count, "Formatting"));
    }

    let outcomes: Vec<Outcome> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
                let next_idx = &next_idx;
                std::thread::Builder::new()
                    .name("format-worker".into())
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        let mut outcomes: Vec<Outcome> = Vec::new();
                        loop {
                            let i = next_idx.fetch_add(1, Ordering::Relaxed);
                            if i >= file_count {
                                break;
                            }
                            if show_progress && i.is_multiple_of(20) {
                                eprint!(
                                    "\r\x1b[2K {}",
                                    progress_bar(i + 1, file_count, "Formatting")
                                );
                            }
                            outcomes.push(format_one(context, &files[i]));
                        }
                        outcomes
                    })
                    .expect("failed to spawn format-worker thread")
            })
            .collect();

        let mut merged: Vec<Outcome> = Vec::new();
        for handle in handles {
            merged.extend(handle.join().unwrap_or_default());
        }
        merged
    });

    if show_progress {
        eprint!(
            "\r\x1b[2K {}\n",
            progress_bar(file_count, file_count, "Formatting")
        );
    }

    let mut reformatted = Vec::new();
    let mut failures = Vec::new();
    for outcome in outcomes {
        match outcome {
            Outcome::Unchanged => {}
            Outcome::Reformatted(file) => reformatted.push(file),
            Outcome::Failed(failure) => failures.push(failure),
        }
    }
    (reformatted, failures)
}

/// Format the file at `path` through whichever strategy applies to it.
fn format_one(context: &Context<'_>, path: &Path) -> Outcome {
    let display_path = path
        .strip_prefix(context.root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return Outcome::Failed(Failure {
                display_path,
                message: format!("failed to read: {e}"),
            });
        }
    };

    // Blade markup isn't PHP, so a template goes through its own strategy
    // — the same split the LSP's formatting handler makes.
    let formatted = if is_blade_path(path) {
        formatting::blade::format_blade_content(
            context.blade_strategy,
            &content,
            path,
            Some(context.root),
            context.config,
            context.blade_options,
            &context.cancelled,
        )
    } else {
        formatting::format_content(
            context.strategy,
            &content,
            path,
            Some(context.root),
            context.config,
            context.php_version,
            &context.cancelled,
        )
    };

    match formatted {
        Ok(None) => Outcome::Unchanged,
        Ok(Some(formatted)) => Outcome::Reformatted(Reformatted {
            display_path,
            abs_path: path.to_path_buf(),
            formatted: context.keep_text.then_some(formatted),
        }),
        Err(message) => Outcome::Failed(Failure {
            display_path,
            message,
        }),
    }
}

/// Whether a discovered file is a Blade template.
///
/// The LSP also honours a `blade` language id on a file named otherwise,
/// which a command-line run has no client to hear it from.
fn is_blade_path(path: &Path) -> bool {
    path.to_str().is_some_and(crate::blade::is_blade_file)
}

/// Name the resolved PHP strategy for the note the run opens with, so it
/// is clear whether a project's own tooling or the built-in formatter
/// produced the result.
fn describe_strategy(strategy: &FormattingStrategy) -> String {
    match strategy {
        FormattingStrategy::Disabled => "formatting disabled".to_string(),
        FormattingStrategy::BuiltIn(_) => "the built-in formatter".to_string(),
        FormattingStrategy::External(tools) => tools
            .iter()
            .map(|resolved| resolved.tool.name())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Name the resolved Blade strategy; see [`describe_strategy`].
fn describe_blade_strategy(strategy: &BladeFormattingStrategy) -> &'static str {
    match strategy {
        BladeFormattingStrategy::Disabled => "formatting disabled",
        BladeFormattingStrategy::BuiltIn => "the built-in reindenter",
        BladeFormattingStrategy::Pint { .. } => Tool::Pint.name(),
    }
}

// ── Output formatting ───────────────────────────────────────────────────────

/// Report the run in the requested output format.
fn report(outcomes: &Outcomes, options: &FormatOptions) {
    let Outcomes {
        reformatted,
        failures,
        ..
    } = outcomes;
    match options.output_format {
        OutputFormat::Table => {
            // Annotate in CI without giving up the readable summary, the
            // way `analyze`, `fix`, and `move` do.
            if std::env::var("GITHUB_ACTIONS").is_ok() {
                print_github_annotations(reformatted, failures, options.check);
            }
            print_table(outcomes, options);
        }
        OutputFormat::Github => print_github_annotations(reformatted, failures, options.check),
        OutputFormat::Json => println!("{}", json_report(reformatted, failures, options.check)),
    }
}

/// List the affected files and close with a summary box.
fn print_table(outcomes: &Outcomes, options: &FormatOptions) {
    let Outcomes {
        reformatted,
        failures,
        file_count,
    } = outcomes;
    for file in reformatted {
        println!(" {}", file.display_path);
    }
    for failure in failures {
        eprintln!("Error: {}: {}", failure.display_path, failure.message);
    }

    if !failures.is_empty() {
        let count = failures.len();
        print_box(
            &format!(" [ERROR] {count} {} could not be formatted ", plural(count)),
            Colour::Red,
            options.use_colour,
        );
        return;
    }

    if reformatted.is_empty() {
        print_success_box(
            &format!(
                " [OK] {file_count} {} already formatted ",
                plural(*file_count)
            ),
            options.use_colour,
        );
        return;
    }

    let count = reformatted.len();
    let label = plural(count);
    if options.check {
        print_box(
            &format!(" [CHECK] {count} {label} would be reformatted "),
            Colour::Yellow,
            options.use_colour,
        );
    } else {
        print_box(
            &format!(" [FORMATTED] Reformatted {count} {label} "),
            Colour::Green,
            options.use_colour,
        );
    }
}

/// `file` or `files`, for a count the summary reads out.
fn plural(count: usize) -> &'static str {
    if count == 1 { "file" } else { "files" }
}

/// The background a summary box is drawn on.
enum Colour {
    Green,
    Yellow,
    Red,
}

/// Print a summary box, matching the ones `analyze` and `fix` close with.
fn print_box(text: &str, colour: Colour, use_colour: bool) {
    if !use_colour {
        println!("{text}");
        return;
    }
    let sgr = match colour {
        Colour::Green => "30;42",
        Colour::Yellow => "30;43",
        Colour::Red => "97;41",
    };
    let pad = " ".repeat(text.len());
    println!();
    println!(" \x1b[{sgr}m{pad}\x1b[0m");
    println!(" \x1b[{sgr}m{text}\x1b[0m");
    println!(" \x1b[{sgr}m{pad}\x1b[0m");
    println!();
}

// ── GitHub Actions annotations ──────────────────────────────────────────────

/// Emit GitHub Actions workflow commands so each file appears as an
/// inline annotation on the pull request diff.
///
/// An unformatted file is an `::error` under `--check`, where it fails the
/// job, and a `::notice` otherwise, where the run already fixed it.
fn print_github_annotations(reformatted: &[Reformatted], failures: &[Failure], check: bool) {
    for line in github_annotations(reformatted, failures, check) {
        println!("{line}");
    }
}

/// The workflow commands [`print_github_annotations`] emits, one per line.
fn github_annotations(
    reformatted: &[Reformatted],
    failures: &[Failure],
    check: bool,
) -> Vec<String> {
    let (level, message) = if check {
        ("error", "File is not formatted. Run `phpantom_lsp format`.")
    } else {
        ("notice", "File was reformatted.")
    };
    reformatted
        .iter()
        .map(|file| {
            format!(
                "::{level} file={path},line=1,col=0,title=format::{message}",
                path = file.display_path,
            )
        })
        .chain(failures.iter().map(|failure| {
            format!(
                "::error file={path},line=1,col=0,title=format::{message}",
                path = failure.display_path,
                message = crate::analyse::format_github_message(&failure.message),
            )
        }))
        .collect()
}

/// The run as a single JSON object.
///
/// ```json
/// {
///   "totals": { "files": 1, "errors": 0, "check": true },
///   "files": ["resources/views/home.blade.php"],
///   "errors": []
/// }
/// ```
fn json_report(reformatted: &[Reformatted], failures: &[Failure], check: bool) -> String {
    let mut out = String::from("{\n");
    let _ = writeln!(
        out,
        "  \"totals\": {{ \"files\": {}, \"errors\": {}, \"check\": {} }},",
        reformatted.len(),
        failures.len(),
        check,
    );

    out.push_str("  \"files\": [");
    for (i, file) in reformatted.iter().enumerate() {
        let _ = write!(
            out,
            "\n    {}{}",
            crate::analyse::json_escape(&file.display_path),
            if i + 1 < reformatted.len() {
                ","
            } else {
                "\n  "
            },
        );
    }
    out.push_str("],\n");

    out.push_str("  \"errors\": [");
    for (i, failure) in failures.iter().enumerate() {
        let _ = write!(
            out,
            "\n    {{ \"file\": {}, \"message\": {} }}{}",
            crate::analyse::json_escape(&failure.display_path),
            crate::analyse::json_escape(&failure.message),
            if i + 1 < failures.len() { "," } else { "\n  " },
        );
    }
    out.push_str("]\n}");
    out
}

#[cfg(test)]
mod tests;
