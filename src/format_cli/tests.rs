//! The `format` command's contract with a CI pipeline: a formatted
//! project passes, an unformatted one fails and names the files, and a
//! run without `--check` leaves the project passing.

use std::path::{Path, PathBuf};

use super::*;

/// A workspace holding `files`, plus the `composer.json` that makes
/// `resources/views` a Blade view directory and `src/` a PSR-4 root.
fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    write_file(
        dir.path(),
        "composer.json",
        r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#,
    );
    for (path, content) in files {
        write_file(dir.path(), path, content);
    }
    dir
}

fn write_file(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("failed to create parent directory");
    }
    std::fs::write(path, content).expect("failed to write file");
}

fn read_file(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).expect("failed to read file")
}

fn options(root: &Path, check: bool) -> FormatOptions {
    FormatOptions {
        workspace_root: root.to_path_buf(),
        path_filters: Vec::new(),
        check,
        indent: "    ".to_string(),
        // No terminal to draw a progress bar on, and no ANSI in an
        // assertion.
        use_colour: false,
        output_format: OutputFormat::Table,
        // Never the machine's own: the run must be decided by the
        // project alone.
        global_config: None,
    }
}

/// The paths `collect` reports as unformatted.
fn unformatted(root: &Path, check: bool) -> Vec<String> {
    let outcomes = collect(&options(root, check)).expect("expected a run");
    assert!(
        outcomes.failures.is_empty(),
        "unexpected failures: {:?}",
        outcomes
            .failures
            .iter()
            .map(|f| format!("{}: {}", f.display_path, f.message))
            .collect::<Vec<_>>()
    );
    outcomes
        .reformatted
        .iter()
        .map(|f| f.display_path.replace('\\', "/"))
        .collect()
}

/// A PHP file the built-in formatter leaves alone, and a Blade template
/// the built-in reindenter leaves alone.
const FORMATTED_PHP: &str = "<?php\n\nnamespace App;\n\nclass Foo {}\n";
const FORMATTED_BLADE: &str = "@if ($x)\n    <p>hello</p>\n@endif\n";

#[test]
fn check_passes_on_a_formatted_project() {
    let dir = workspace(&[
        ("src/Foo.php", FORMATTED_PHP),
        ("resources/views/home.blade.php", FORMATTED_BLADE),
    ]);

    assert_eq!(unformatted(dir.path(), true), Vec::<String>::new());
    assert_eq!(run(options(dir.path(), true)), 0);
}

#[test]
fn check_names_an_unformatted_blade_template_without_writing_it() {
    let unindented = "@if ($x)\n<p>hello</p>\n@endif\n";
    let dir = workspace(&[
        ("src/Foo.php", FORMATTED_PHP),
        ("resources/views/home.blade.php", unindented),
    ]);

    assert_eq!(
        unformatted(dir.path(), true),
        vec!["resources/views/home.blade.php".to_string()]
    );
    assert_eq!(run(options(dir.path(), true)), 2);
    assert_eq!(
        read_file(dir.path(), "resources/views/home.blade.php"),
        unindented,
        "--check must not write"
    );
}

#[test]
fn check_names_an_unformatted_php_file_without_writing_it() {
    let unformatted_php = "<?php\nnamespace App;\nclass Foo    {}\n";
    let dir = workspace(&[("src/Foo.php", unformatted_php)]);

    assert_eq!(
        unformatted(dir.path(), true),
        vec!["src/Foo.php".to_string()]
    );
    assert_eq!(run(options(dir.path(), true)), 2);
    assert_eq!(
        read_file(dir.path(), "src/Foo.php"),
        unformatted_php,
        "--check must not write"
    );
}

#[test]
fn formatting_rewrites_the_files_and_a_second_run_is_a_no_op() {
    let dir = workspace(&[
        ("src/Foo.php", "<?php\nnamespace App;\nclass Foo    {}\n"),
        (
            "resources/views/home.blade.php",
            "@if ($x)\n<p>hi</p>\n@endif\n",
        ),
    ]);

    assert_eq!(run(options(dir.path(), false)), 0);
    assert_eq!(
        read_file(dir.path(), "resources/views/home.blade.php"),
        "@if ($x)\n    <p>hi</p>\n@endif\n"
    );
    let php = read_file(dir.path(), "src/Foo.php");
    assert_ne!(php, "<?php\nnamespace App;\nclass Foo    {}\n");

    // The point of the command: once it has run, `--check` passes.
    assert_eq!(unformatted(dir.path(), true), Vec::<String>::new());
    assert_eq!(run(options(dir.path(), false)), 0);
    assert_eq!(read_file(dir.path(), "src/Foo.php"), php);
}

#[test]
fn a_path_filter_restricts_the_run() {
    let dir = workspace(&[
        ("src/Foo.php", "<?php\nnamespace App;\nclass Foo    {}\n"),
        (
            "resources/views/home.blade.php",
            "@if ($x)\n<p>hi</p>\n@endif\n",
        ),
    ]);

    let mut opts = options(dir.path(), true);
    opts.path_filters = vec![dir.path().join("resources")];
    let outcomes = collect(&opts).expect("expected a run");

    assert_eq!(
        outcomes
            .reformatted
            .iter()
            .map(|f| f.display_path.replace('\\', "/"))
            .collect::<Vec<_>>(),
        vec!["resources/views/home.blade.php".to_string()],
        "the filtered-out PHP file is unformatted too, but out of scope"
    );
}

#[test]
fn a_template_whose_whitespace_is_output_is_left_alone() {
    // A mail template's leading spaces are Markdown, so no formatter may
    // touch them; `--check` must not fail a project for having one.
    let dir = workspace(&[(
        "resources/views/mail/welcome.blade.php",
        "@component('mail::message')\n<p>hello</p>\n@endcomponent\n",
    )]);

    assert_eq!(unformatted(dir.path(), true), Vec::<String>::new());
}

#[test]
fn formatting_disabled_in_config_reports_no_run_at_all() {
    let dir = workspace(&[("src/Foo.php", "<?php\nnamespace App;\nclass Foo    {}\n")]);
    write_file(
        dir.path(),
        ".phpantom.toml",
        "[formatting]\npint = \"\"\nphp-cs-fixer = \"\"\nphpcbf = \"\"\n",
    );

    assert!(
        collect(&options(dir.path(), true)).is_none(),
        "a disabled formatter has nothing to enforce"
    );
    assert_eq!(run(options(dir.path(), true)), 0);
}

// ── Machine-readable output ─────────────────────────────────────────

fn sample() -> (Vec<Reformatted>, Vec<Failure>) {
    (
        vec![Reformatted {
            display_path: "resources/views/home.blade.php".to_string(),
            abs_path: PathBuf::from("/ws/resources/views/home.blade.php"),
            formatted: None,
        }],
        vec![Failure {
            display_path: "src/Foo.php".to_string(),
            message: "pint failed".to_string(),
        }],
    )
}

#[test]
fn check_annotates_an_unformatted_file_as_an_error() {
    let (reformatted, _) = sample();
    assert_eq!(
        github_annotations(&reformatted, &[], true),
        vec![
            "::error file=resources/views/home.blade.php,line=1,col=0,title=format::File is not formatted. Run `phpantom_lsp format`.".to_string(),
        ]
    );
}

#[test]
fn a_write_run_annotates_a_reformatted_file_as_a_notice() {
    let (reformatted, _) = sample();
    assert_eq!(
        github_annotations(&reformatted, &[], false),
        vec![
            "::notice file=resources/views/home.blade.php,line=1,col=0,title=format::File was reformatted.".to_string(),
        ]
    );
}

#[test]
fn a_failure_is_always_an_error_annotation() {
    let (_, failures) = sample();
    assert_eq!(
        github_annotations(&[], &failures, false),
        vec!["::error file=src/Foo.php,line=1,col=0,title=format::pint failed".to_string(),]
    );
}

#[test]
fn json_lists_the_files_and_the_failures() {
    let (reformatted, failures) = sample();
    assert_eq!(
        json_report(&reformatted, &failures, true),
        r#"{
  "totals": { "files": 1, "errors": 1, "check": true },
  "files": [
    "resources/views/home.blade.php"
  ],
  "errors": [
    { "file": "src/Foo.php", "message": "pint failed" }
  ]
}"#
    );
}

#[test]
fn json_of_a_clean_run_has_empty_lists() {
    assert_eq!(
        json_report(&[], &[], false),
        r#"{
  "totals": { "files": 0, "errors": 0, "check": false },
  "files": [],
  "errors": []
}"#
    );
}
