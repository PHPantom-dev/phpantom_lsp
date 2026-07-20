//! CLI fix mode.
//!
//! Applies automated code fixes across a PHP project, modeled after
//! php-cs-fixer.  Each "rule" corresponds to a diagnostic code (e.g.
//! `unused_import`) and its associated code action.
//!
//! # Usage
//!
//! ```sh
//! phpantom_lsp fix                              # apply all preferred native fixers
//! phpantom_lsp fix --rule unused_import          # only remove unused imports
//! phpantom_lsp fix --rule unused_import --rule deprecated  # multiple rules
//! phpantom_lsp fix --dry-run                     # show what would change without writing
//! phpantom_lsp fix src/                          # restrict to a subdirectory
//! phpantom_lsp fix src/Foo.php                   # fix a single file
//! ```
//!
//! # Design
//!
//! The fixer pipeline is:
//!
//! 1. **Init** — same headless `Backend` setup as `analyse`.
//! 2. **Discover** — reuses `analyse::discover_user_files`.
//! 3. **Parse** — parallel `update_ast` pass (identical to analyse Phase 1).
//! 4. **Fix** — for each file, run the selected diagnostic collectors,
//!    compute the corresponding code-action edits, and apply them.
//! 5. **Write** — write modified files back to disk (unless `--dry-run`).
//!
//! Rules are identified by their diagnostic code string. Native rules
//! use bare identifiers (`unused_import`, `deprecated`). PHPStan-based
//! rules use a `phpstan.` prefix (`phpstan.return.unusedType`). PHPStan
//! rules are only available when `--with-phpstan` is passed.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use tower_lsp::lsp_types::*;

use crate::analyse::OutputFormat;
use crate::code_actions::build_line_deletion_edit;
use crate::parser::with_parse_cache;
use crate::util::position_to_byte_offset;
use crate::virtual_members::with_active_resolved_class_cache;
use crate::{Backend, composer, config};

/// Options for the fix command.
#[derive(Debug)]
pub struct FixOptions {
    /// Workspace root.  Usually a Composer project directory; a plain
    /// PHP tree without a composer.json is fixed by walking the root.
    pub workspace_root: PathBuf,
    /// Optional path filter: only fix files under this path.
    pub path_filter: Option<PathBuf>,
    /// Specific rules to apply. Empty means "all preferred native rules".
    pub rules: Vec<String>,
    /// When true, report what would change but do not write files.
    pub dry_run: bool,
    /// Whether to output with ANSI colours.
    pub use_colour: bool,
    /// Whether to run PHPStan-based fixers (requires `--with-phpstan`).
    pub with_phpstan: bool,
    /// Output format.
    pub output_format: OutputFormat,
}

/// A single fix applied to a file.
struct AppliedFix {
    /// The rule that produced this fix (diagnostic code).
    rule: String,
    /// 1-based line number where the fix was applied.
    line: u32,
    /// Human-readable description of what was fixed.
    description: String,
}

/// Summary of fixes for one file.
struct FileFixResult {
    /// Display path (relative to workspace root).
    display_path: String,
    /// Absolute path for writing back.
    abs_path: PathBuf,
    /// The new file content after all fixes.
    new_content: String,
    /// Whether the content actually changed.
    changed: bool,
    /// Individual fixes applied.
    fixes: Vec<AppliedFix>,
}

/// All native rule identifiers that have automated fixers.
const NATIVE_RULES: &[&str] = &["unused_import"];

/// Check whether a rule identifier refers to a PHPStan-based fixer.
fn is_phpstan_rule(rule: &str) -> bool {
    rule.starts_with("phpstan.")
}

/// Validate that all requested rules are known. Returns an error message
/// for each unknown rule, or an empty vec if all are valid.
fn validate_rules(rules: &[String], with_phpstan: bool) -> Vec<String> {
    let mut errors = Vec::new();
    for rule in rules {
        if is_phpstan_rule(rule) {
            if !with_phpstan {
                errors.push(format!(
                    "Rule '{rule}' requires --with-phpstan to be enabled"
                ));
            }
            // PHPStan rules are validated at runtime against actual
            // diagnostic codes; we don't maintain a static list here.
        } else if !NATIVE_RULES.contains(&rule.as_str()) {
            errors.push(format!("Unknown rule: '{rule}'"));
        }
    }
    errors
}

/// Determine which native rules to run based on options.
fn effective_native_rules(rules: &[String]) -> Vec<&'static str> {
    if rules.is_empty() {
        // No rules specified: run all preferred native fixers.
        NATIVE_RULES.to_vec()
    } else {
        // Filter to only the native rules that were requested.
        NATIVE_RULES
            .iter()
            .filter(|r| rules.iter().any(|req| req == **r))
            .copied()
            .collect()
    }
}

/// Apply unused-import fixes to a single file.
///
/// Returns the modified content and a list of fixes applied.
fn fix_unused_imports(backend: &Backend, uri: &str, content: &str) -> (String, Vec<AppliedFix>) {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    backend.collect_unused_import_diagnostics(uri, content, &mut diagnostics);

    if diagnostics.is_empty() {
        return (content.to_string(), Vec::new());
    }

    // Collect which import lines are being removed (for blank-line
    // collapsing logic).
    let removed_import_lines: HashSet<usize> = diagnostics
        .iter()
        .map(|d| d.range.start.line as usize)
        .collect();

    // Build deletion edits for each unused import.
    let mut edits: Vec<TextEdit> = diagnostics
        .iter()
        .map(|d| build_line_deletion_edit(content, &d.range, &removed_import_lines))
        .collect();

    // Sort edits in reverse order so byte offsets remain valid as we
    // apply deletions from bottom to top.
    edits.sort_by_key(|b| std::cmp::Reverse(b.range.start));

    // Record what we fixed before applying edits.
    let fixes: Vec<AppliedFix> = diagnostics
        .iter()
        .map(|d| AppliedFix {
            rule: "unused_import".to_string(),
            line: d.range.start.line + 1,
            description: d.message.clone(),
        })
        .collect();

    // Apply edits to content.
    let new_content = apply_text_edits(content, &edits);

    (new_content, fixes)
}

/// Apply a sorted (reverse order) list of non-overlapping `TextEdit`s
/// to a string, returning the modified content.
fn apply_text_edits(content: &str, edits: &[TextEdit]) -> String {
    let mut result = content.to_string();

    for edit in edits {
        let start = position_to_byte_offset(&result, edit.range.start);
        let end = position_to_byte_offset(&result, edit.range.end);

        if start <= end && end <= result.len() {
            result.replace_range(start..end, &edit.new_text);
        }
    }

    result
}

/// Run the fix command and return the process exit code.
///
/// Returns `0` when fixes were applied (or nothing to fix), `1` on error,
/// `2` when `--dry-run` found fixable issues.
pub async fn run(options: FixOptions) -> i32 {
    let root = &options.workspace_root;

    // A missing composer.json is not an error: plain PHP trees fix
    // fine — classes are indexed by scanning the tree and files are
    // discovered by walking the root.  Note it on stderr so a mistyped
    // --project-root does not silently fix the wrong directory.
    if !root.join("composer.json").is_file() {
        eprintln!(
            "Note: no composer.json found in {} — treating it as a plain PHP project.",
            root.display()
        );
    }

    // ── Validate rules ──────────────────────────────────────────────
    let rule_errors = validate_rules(&options.rules, options.with_phpstan);
    if !rule_errors.is_empty() {
        for err in &rule_errors {
            eprintln!("Error: {err}");
        }
        return 1;
    }

    let native_rules = effective_native_rules(&options.rules);
    if native_rules.is_empty() && !options.with_phpstan {
        eprintln!("No applicable rules to run.");
        return 0;
    }

    // ── 1. Load config ──────────────────────────────────────────────
    let cfg = match config::load_config(root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: failed to load .phpantom.toml: {e}");
            config::Config::default()
        }
    };

    // ── 2. Index project ────────────────────────────────────────────
    let backend = Backend::new_headless();
    *backend.workspace_root().write() = Some(root.to_path_buf());
    *backend.config.lock() = cfg.clone();

    let composer_package = composer::read_composer_package(root);

    let php_version = cfg
        .php
        .version
        .as_deref()
        .and_then(crate::types::PhpVersion::from_composer_constraint)
        .unwrap_or_else(|| {
            composer_package
                .as_ref()
                .and_then(composer::detect_php_version_from_package)
                .unwrap_or_default()
        });
    backend.set_php_version(php_version);

    backend
        .init_single_project(root, php_version, composer_package, None)
        .await;

    // ── 3. Discover files ───────────────────────────────────────────
    let files = crate::analyse::discover_user_files(&backend, root, options.path_filter.as_deref());

    if files.is_empty() {
        eprintln!("No PHP files found.");
        return 0;
    }

    let file_count = files.len();
    let use_colour = options.use_colour;
    let output_format = options.output_format;
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // ── Phase 1: Parse all files (parallel) ─────────────────────────
    if use_colour && output_format == OutputFormat::Table {
        eprint!("\r\x1b[2K {}", progress_bar(0, file_count, "Parsing"));
    }
    let next_idx = AtomicUsize::new(0);

    let file_data: Vec<Option<(String, String, PathBuf)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
                let backend = &backend;
                let next_idx = &next_idx;
                let files = &files;
                s.spawn(move || {
                    let mut entries: Vec<(usize, String, String, PathBuf)> = Vec::new();
                    loop {
                        let i = next_idx.fetch_add(1, Ordering::Relaxed);
                        if i >= file_count {
                            break;
                        }

                        let file_path = &files[i];
                        let content = match std::fs::read_to_string(file_path) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };

                        let uri = crate::util::path_to_uri(file_path);
                        backend.update_ast(&uri, &content);
                        entries.push((i, uri, content, file_path.clone()));
                    }
                    entries
                })
            })
            .collect();

        let mut indexed: Vec<Option<(String, String, PathBuf)>> =
            (0..file_count).map(|_| None).collect();
        for handle in handles {
            for (i, uri, content, path) in handle.join().unwrap_or_default() {
                indexed[i] = Some((uri, content, path));
            }
        }
        indexed
    });

    if use_colour && output_format == OutputFormat::Table {
        eprint!(
            "\r\x1b[2K {}\n",
            progress_bar(file_count, file_count, "Parsing")
        );
    }

    // Discover the configured Laravel date class now that every user file
    // is parsed, so the `now()`/`today()` helpers and the Date facade
    // resolve to a concrete class instead of nothing.  Matches the LSP
    // `initialized` handler and the `analyze` pipeline; without it, fixes
    // could be driven by false-positive return-type diagnostics.
    if backend.resolved_class_cache.read().is_laravel() {
        backend.build_laravel_date_class();
    }

    // ── Phase 2: Fix files (parallel) ───────────────────────────────
    if use_colour && output_format == OutputFormat::Table {
        eprint!("\r\x1b[2K {}", progress_bar(0, file_count, "Fixing"));
    }
    let next_idx = AtomicUsize::new(0);
    let dry_run = options.dry_run;

    let results: Vec<FileFixResult> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
                let backend = &backend;
                let next_idx = &next_idx;
                let file_data = &file_data;
                let files = &files;
                let native_rules = &native_rules;
                s.spawn(move || {
                    let mut results: Vec<FileFixResult> = Vec::new();
                    loop {
                        let i = next_idx.fetch_add(1, Ordering::Relaxed);
                        if i >= file_count {
                            break;
                        }
                        if use_colour
                            && output_format == OutputFormat::Table
                            && i.is_multiple_of(20)
                        {
                            eprint!("\r\x1b[2K {}", progress_bar(i + 1, file_count, "Fixing"));
                        }

                        let (uri, content, abs_path) = match &file_data[i] {
                            Some(tuple) => (&tuple.0, &tuple.1, &tuple.2),
                            None => continue,
                        };

                        let _parse_guard = with_parse_cache(content);
                        let _cache_guard =
                            with_active_resolved_class_cache(&backend.resolved_class_cache);

                        let mut current_content = content.clone();
                        let mut all_fixes: Vec<AppliedFix> = Vec::new();

                        // Apply each rule in order.
                        for rule in native_rules.iter() {
                            match *rule {
                                "unused_import" => {
                                    let (new_content, fixes) =
                                        fix_unused_imports(backend, uri, &current_content);
                                    current_content = new_content;
                                    all_fixes.extend(fixes);
                                }
                                _ => {
                                    // Future rules go here.
                                }
                            }
                        }

                        let changed = current_content != *content;
                        if changed {
                            let display_path = files[i]
                                .strip_prefix(root)
                                .unwrap_or(&files[i])
                                .to_string_lossy()
                                .to_string();
                            results.push(FileFixResult {
                                display_path,
                                abs_path: abs_path.clone(),
                                new_content: current_content,
                                changed,
                                fixes: all_fixes,
                            });
                        }
                    }
                    results
                })
            })
            .collect();

        let mut merged: Vec<FileFixResult> = Vec::new();
        for handle in handles {
            merged.extend(handle.join().unwrap_or_default());
        }
        merged
    });

    if use_colour && output_format == OutputFormat::Table {
        eprint!(
            "\r\x1b[2K {}\n",
            progress_bar(file_count, file_count, "Fixing")
        );
    }

    // ── Phase 3: Write results ──────────────────────────────────────
    let mut sorted_results: Vec<FileFixResult> =
        results.into_iter().filter(|r| r.changed).collect();
    sorted_results.sort_by(|a, b| a.display_path.cmp(&b.display_path));

    if sorted_results.is_empty() {
        match output_format {
            OutputFormat::Table => print_success_box(use_colour),
            OutputFormat::Github => {} // no output on success
            OutputFormat::Json => print_fix_json(&[], 0, dry_run),
        }
        return 0;
    }

    let total_fixes: usize = sorted_results.iter().map(|r| r.fixes.len()).sum();
    let files_changed = sorted_results.len();

    match output_format {
        OutputFormat::Table => {
            // When running in GitHub Actions, also emit annotations
            // alongside the table (same behaviour as PHPStan).
            if std::env::var("GITHUB_ACTIONS").is_ok() {
                print_fix_github_annotations(&sorted_results);
            }
            for result in &sorted_results {
                print_fix_table(&result.display_path, &result.fixes, use_colour);
            }
        }
        OutputFormat::Github => {
            print_fix_github_annotations(&sorted_results);
        }
        OutputFormat::Json => {
            print_fix_json(&sorted_results, total_fixes, dry_run);
        }
    }

    if dry_run {
        if output_format == OutputFormat::Table {
            print_dry_run_box(total_fixes, files_changed, use_colour);
        }
        return 2;
    }

    // Write files.
    let mut write_errors = 0;
    for result in &sorted_results {
        if let Err(e) = std::fs::write(&result.abs_path, &result.new_content) {
            eprintln!("Error: failed to write {}: {e}", result.display_path);
            write_errors += 1;
        }
    }

    if write_errors > 0 {
        eprintln!("{write_errors} file(s) failed to write.");
        return 1;
    }

    if output_format == OutputFormat::Table {
        print_fixed_box(total_fixes, files_changed, use_colour);
    }

    0
}

// ── Output formatting ───────────────────────────────────────────────────────

/// Print a file's fixes in a table format.
fn print_fix_table(path: &str, fixes: &[AppliedFix], use_colour: bool) {
    let line_col_w = fixes
        .iter()
        .map(|f| f.line.to_string().len())
        .max()
        .unwrap_or(0)
        .max(4);

    let msg_col_w = fixes
        .iter()
        .map(|f| f.description.len())
        .max()
        .unwrap_or(0)
        .max(path.len());

    let sep = format!(
        " {} {}",
        "-".repeat(line_col_w + 2),
        "-".repeat(msg_col_w + 2),
    );

    println!("{sep}");
    if use_colour {
        println!(
            "  \x1b[32m{:>line_col_w$}\x1b[0m   \x1b[32m{path}\x1b[0m",
            "Line"
        );
    } else {
        println!("  {:>line_col_w$}   {path}", "Line");
    }
    println!("{sep}");

    for fix in fixes {
        let line_str = fix.line.to_string();
        println!("  {:>line_col_w$}   {}", line_str, fix.description);
        if use_colour {
            println!(
                "  {:>line_col_w$}   \x1b[2m\u{1f527}  {}\x1b[0m",
                "", fix.rule
            );
        } else {
            println!("  {:>line_col_w$}   \u{1f527}  {}", "", fix.rule);
        }
    }

    println!("{sep}");
    println!();
}

/// Print the success box (nothing to fix).
fn print_success_box(use_colour: bool) {
    let text = " [OK] No fixable issues found ";
    if use_colour {
        let pad = " ".repeat(text.len());
        println!();
        println!(" \x1b[30;42m{pad}\x1b[0m");
        println!(" \x1b[30;42m{text}\x1b[0m");
        println!(" \x1b[30;42m{pad}\x1b[0m");
        println!();
    } else {
        println!("{text}");
    }
}

/// Print the dry-run summary box.
fn print_dry_run_box(total_fixes: usize, files_changed: usize, use_colour: bool) {
    let fix_label = if total_fixes == 1 { "fix" } else { "fixes" };
    let file_label = if files_changed == 1 { "file" } else { "files" };
    let text = format!(
        " [DRY RUN] {total_fixes} {fix_label} in {files_changed} {file_label} (not applied) "
    );
    if use_colour {
        let pad = " ".repeat(text.len());
        println!();
        println!(" \x1b[30;43m{pad}\x1b[0m");
        println!(" \x1b[30;43m{text}\x1b[0m");
        println!(" \x1b[30;43m{pad}\x1b[0m");
        println!();
    } else {
        println!("{text}");
    }
}

/// Print the fixed summary box.
fn print_fixed_box(total_fixes: usize, files_changed: usize, use_colour: bool) {
    let fix_label = if total_fixes == 1 { "fix" } else { "fixes" };
    let file_label = if files_changed == 1 { "file" } else { "files" };
    let text =
        format!(" [FIXED] Applied {total_fixes} {fix_label} across {files_changed} {file_label} ");
    if use_colour {
        let pad = " ".repeat(text.len());
        println!();
        println!(" \x1b[30;42m{pad}\x1b[0m");
        println!(" \x1b[30;42m{text}\x1b[0m");
        println!(" \x1b[30;42m{pad}\x1b[0m");
        println!();
    } else {
        println!("{text}");
    }
}

// ── Progress bar ────────────────────────────────────────────────────────────

// ── GitHub Actions annotations ──────────────────────────────────────────────

/// Emit GitHub Actions workflow commands for fix results.
///
/// Each fix is printed as a `::notice` annotation so it appears as an
/// inline annotation on pull request diffs.
fn print_fix_github_annotations(results: &[FileFixResult]) {
    for result in results {
        for fix in &result.fixes {
            let message = crate::analyse::format_github_message(&fix.description);
            println!(
                "::notice file={path},line={line},col=0,title={rule}::{message}",
                path = result.display_path,
                line = fix.line,
                rule = fix.rule,
            );
        }
    }
}

/// Print fix results as a single JSON object.
///
/// ```json
/// {
///   "totals": { "fixes": 3, "dry_run": false },
///   "files": {
///     "src/Foo.php": {
///       "fixes": 1,
///       "changes": [
///         { "line": 5, "rule": "unused_import", "description": "..." }
///       ]
///     }
///   }
/// }
/// ```
fn print_fix_json(results: &[FileFixResult], total_fixes: usize, dry_run: bool) {
    let mut out = String::from("{\n");
    let _ = writeln!(
        out,
        "  \"totals\": {{ \"fixes\": {}, \"dry_run\": {} }},",
        total_fixes, dry_run
    );

    if results.is_empty() {
        out.push_str("  \"files\": {}\n");
    } else {
        out.push_str("  \"files\": {\n");
        for (i, result) in results.iter().enumerate() {
            let _ = write!(
                out,
                "    {}: {{\n      \"fixes\": {},\n      \"changes\": [\n",
                crate::analyse::json_escape(&result.display_path),
                result.fixes.len()
            );
            for (j, fix) in result.fixes.iter().enumerate() {
                let _ = write!(
                    out,
                    "        {{ \"line\": {}, \"rule\": {}, \"description\": {} }}",
                    fix.line,
                    crate::analyse::json_escape(&fix.rule),
                    crate::analyse::json_escape(&fix.description),
                );
                if j + 1 < result.fixes.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("      ]\n    }");
            if i + 1 < results.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  }\n");
    }

    out.push('}');
    println!("{out}");
}

const BAR_WIDTH: usize = 28;

/// Render a progress bar with a phase label.
fn progress_bar(done: usize, total: usize, label: &str) -> String {
    let pct = (done * 100).checked_div(total).unwrap_or(100);
    let filled = (done * BAR_WIDTH).checked_div(total).unwrap_or(BAR_WIDTH);
    let empty = BAR_WIDTH - filled;

    format!(
        " {done:>width$}/{total} [{bar_fill}{bar_empty}] {pct:>3}% {label}",
        width = total.to_string().len(),
        bar_fill = "\u{2593}".repeat(filled),
        bar_empty = "\u{2591}".repeat(empty),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Re-import for tests that call position_to_byte_offset directly.
    use crate::util::position_to_byte_offset as lsp_position_to_byte_offset;

    #[test]
    fn apply_text_edits_removes_lines_bottom_to_top() {
        let content = "line 0\nline 1\nline 2\nline 3\n";
        // Remove line 2 and line 0 (reverse order).
        let edits = vec![
            TextEdit {
                range: Range {
                    start: Position::new(2, 0),
                    end: Position::new(3, 0),
                },
                new_text: String::new(),
            },
            TextEdit {
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(1, 0),
                },
                new_text: String::new(),
            },
        ];
        let result = apply_text_edits(content, &edits);
        assert_eq!(result, "line 1\nline 3\n");
    }

    #[test]
    fn apply_text_edits_empty_list_returns_unchanged() {
        let content = "unchanged\n";
        let result = apply_text_edits(content, &[]);
        assert_eq!(result, content);
    }

    #[test]
    fn position_to_byte_offset_first_line() {
        let content = "hello world\nsecond line\n";
        assert_eq!(lsp_position_to_byte_offset(content, Position::new(0, 0)), 0);
        assert_eq!(lsp_position_to_byte_offset(content, Position::new(0, 5)), 5);
    }

    #[test]
    fn position_to_byte_offset_second_line() {
        let content = "hello\nworld\n";
        // "world" starts at byte 6.
        assert_eq!(lsp_position_to_byte_offset(content, Position::new(1, 0)), 6);
        assert_eq!(lsp_position_to_byte_offset(content, Position::new(1, 3)), 9);
    }

    #[test]
    fn position_to_byte_offset_past_end() {
        let content = "abc";
        assert_eq!(lsp_position_to_byte_offset(content, Position::new(5, 0)), 3);
    }

    #[test]
    fn validate_rules_accepts_known_native_rules() {
        let errors = validate_rules(&["unused_import".to_string()], false);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_rules_rejects_unknown_rules() {
        let errors = validate_rules(&["nonexistent_rule".to_string()], false);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Unknown rule"));
    }

    #[test]
    fn validate_rules_rejects_phpstan_without_flag() {
        let errors = validate_rules(&["phpstan.return.unusedType".to_string()], false);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("--with-phpstan"));
    }

    #[test]
    fn validate_rules_accepts_phpstan_with_flag() {
        let errors = validate_rules(&["phpstan.return.unusedType".to_string()], true);
        assert!(errors.is_empty());
    }

    #[test]
    fn effective_native_rules_empty_returns_all() {
        let rules = effective_native_rules(&[]);
        assert_eq!(rules, NATIVE_RULES);
    }

    #[test]
    fn effective_native_rules_filters_to_requested() {
        let rules = effective_native_rules(&["unused_import".to_string()]);
        assert_eq!(rules, vec!["unused_import"]);
    }

    #[test]
    fn effective_native_rules_ignores_phpstan_rules() {
        let rules = effective_native_rules(&["phpstan.return.unusedType".to_string()]);
        assert!(rules.is_empty());
    }

    #[test]
    fn is_phpstan_rule_with_prefix() {
        assert!(is_phpstan_rule("phpstan.return.unusedType"));
        assert!(is_phpstan_rule("phpstan.anything"));
    }

    #[test]
    fn is_phpstan_rule_without_prefix() {
        assert!(!is_phpstan_rule("unused_import"));
        assert!(!is_phpstan_rule("deprecated_usage"));
    }

    // ── End-to-end through fix_unused_imports ────────────────────────

    #[test]
    fn fix_removes_middle_import_without_blank_line() {
        // Reproduces the user-reported bug: removing `use PHPMD\Rule;`
        // from a contiguous block left a blank line between survivors.
        let backend = crate::Backend::new_test();
        let content = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;
use PHPMD\\Rule\\Design\\CouplingBetweenObjects;

class Foo extends AbstractCallableNode {
    public function bar(MethodNode $m, CouplingBetweenObjects $c): void {}
}
";
        let uri = "file:///test.php";
        backend.update_ast(uri, content);
        let (result, fixes) = fix_unused_imports(&backend, uri, content);

        assert_eq!(fixes.len(), 1, "should fix exactly one unused import");
        assert!(
            fixes[0].description.contains("Rule"),
            "should fix the Rule import"
        );

        let expected = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule\\Design\\CouplingBetweenObjects;

class Foo extends AbstractCallableNode {
    public function bar(MethodNode $m, CouplingBetweenObjects $c): void {}
}
";
        assert_eq!(
            result, expected,
            "Removing a middle import should not leave a blank line"
        );
    }

    #[test]
    fn fix_removes_first_import_without_blank_line() {
        let backend = crate::Backend::new_test();
        let content = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;

class Foo {
    public function bar(MethodNode $m, Rule $r): void {}
}
";
        let uri = "file:///test.php";
        backend.update_ast(uri, content);
        let (result, fixes) = fix_unused_imports(&backend, uri, content);

        assert_eq!(fixes.len(), 1);
        assert!(fixes[0].description.contains("AbstractCallableNode"));

        let expected = "\
<?php
namespace Test;

use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;

class Foo {
    public function bar(MethodNode $m, Rule $r): void {}
}
";
        assert_eq!(
            result, expected,
            "Removing the first import should not leave a blank line"
        );
    }

    #[test]
    fn fix_removes_last_import_without_blank_line() {
        let backend = crate::Backend::new_test();
        let content = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;

class Foo {
    public function bar(AbstractCallableNode $a, MethodNode $m): void {}
}
";
        let uri = "file:///test.php";
        backend.update_ast(uri, content);
        let (result, fixes) = fix_unused_imports(&backend, uri, content);

        assert_eq!(fixes.len(), 1);
        assert!(fixes[0].description.contains("Rule"));

        let expected = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;

class Foo {
    public function bar(AbstractCallableNode $a, MethodNode $m): void {}
}
";
        assert_eq!(
            result, expected,
            "Removing the last import should not leave a blank line"
        );
    }
}
