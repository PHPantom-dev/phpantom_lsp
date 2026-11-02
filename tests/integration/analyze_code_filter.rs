//! `analyze --code` and `--exclude-code` choose which diagnostics decide the
//! run, alongside `--severity`.

use std::fs;

use phpantom_lsp::analyse::{AnalyseOptions, CodeFilter, OutputFormat, SeverityFilter, run};

const UNKNOWN_FUNCTION: &str = "unknown_function";
const UNUSED_IMPORT: &str = "unused_import";

/// A project reporting exactly one `unknown_function` error and one
/// `unused_import` hint.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.php"),
        "<?php\nnamespace App;\nuse Exception;\nnot_a_function();\n",
    )
    .unwrap();
    dir
}

async fn exit_code(only: &[&str], excluded: &[&str], severity: SeverityFilter) -> i32 {
    let owned = |codes: &[&str]| codes.iter().map(ToString::to_string).collect();
    let dir = project();
    run(AnalyseOptions {
        workspace_root: dir.path().to_path_buf(),
        path_filters: Vec::new(),
        severity_filter: severity,
        code_filter: CodeFilter::new(owned(only), owned(excluded)),
        use_colour: false,
        output_format: OutputFormat::Json,
        // Suppresses the progress bar, which has no terminal to draw on here.
        debug: true,
        verbosity: 0,
        // Never the machine's own; the test asserts on the project alone.
        global_config: None,
    })
    .await
}

#[tokio::test]
async fn a_code_that_is_reported_fails_the_run() {
    assert_eq!(
        exit_code(&[UNKNOWN_FUNCTION], &[], SeverityFilter::All).await,
        1
    );
}

#[tokio::test]
async fn a_code_the_project_does_not_trigger_passes_the_run() {
    assert_eq!(
        exit_code(&["syntax_error"], &[], SeverityFilter::All).await,
        0
    );
}

#[tokio::test]
async fn excluding_one_code_keeps_the_others() {
    assert_eq!(
        exit_code(&[], &[UNUSED_IMPORT], SeverityFilter::All).await,
        1
    );
}

#[tokio::test]
async fn excluding_every_reported_code_passes_the_run() {
    assert_eq!(
        exit_code(&[], &[UNKNOWN_FUNCTION, UNUSED_IMPORT], SeverityFilter::All).await,
        0
    );
}

#[tokio::test]
async fn an_exclusion_wins_over_the_same_code_asked_for() {
    assert_eq!(
        exit_code(
            &[UNKNOWN_FUNCTION],
            &[UNKNOWN_FUNCTION],
            SeverityFilter::All
        )
        .await,
        0
    );
}

#[tokio::test]
async fn the_code_filter_applies_on_top_of_the_severity_filter() {
    assert_eq!(
        exit_code(&[UNUSED_IMPORT], &[], SeverityFilter::All).await,
        1
    );
    assert_eq!(
        exit_code(&[UNUSED_IMPORT], &[], SeverityFilter::Error).await,
        0
    );
}
