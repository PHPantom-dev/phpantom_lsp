//! Output formatting for the `move` command.
//!
//! The three formats match `analyze` and `fix` so a script driving a
//! batch of refactors reads all three the same way: a human-readable
//! summary, GitHub Actions workflow annotations, and a JSON object
//! shaped like the one `analyze` emits.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::analyse::{
    JsonFileEntry, JsonMessage, JsonTotals, format_github_message, github_annotation,
};

use super::MoveSummary;

/// The diagnostic identifier every move warning carries, so a consumer
/// filtering `analyze` output by identifier can filter these too.
const IDENTIFIER: &str = "move_incomplete";

/// Print the human-readable summary on stdout and every warning on
/// stderr.
pub(super) fn print_table(summary: &MoveSummary, use_colour: bool) {
    let verb = if summary.dry_run {
        "Would move"
    } else {
        "Moved"
    };
    println!(
        "{verb} {} `{}` to `{}` ({} file(s) changed, {} path(s) moved).",
        summary.kind, summary.from, summary.to, summary.files_changed, summary.paths_moved
    );

    let label = if use_colour {
        "\x1b[33mWarning:\x1b[0m"
    } else {
        "Warning:"
    };
    for warning in &summary.warnings {
        let Some(file) = warning.file.as_deref() else {
            eprintln!("{label} {}", warning.message);
            continue;
        };
        let location = match warning.line {
            Some(line) => format!("{file}:{line}:"),
            None => format!("{file}:"),
        };
        if use_colour {
            eprintln!("{label} \x1b[2m{location}\x1b[0m {}", warning.message);
        } else {
            eprintln!("{label} {location} {}", warning.message);
        }
    }
}

/// Emit each warning as a GitHub Actions workflow command so a CI job
/// annotates the lines a move could not reach.
pub(super) fn print_github_annotations(summary: &MoveSummary) {
    for warning in &summary.warnings {
        match &warning.file {
            Some(file) => println!(
                "{}",
                github_annotation(
                    "warning",
                    file,
                    warning.line.unwrap_or(1),
                    IDENTIFIER,
                    &warning.message,
                )
            ),
            None => println!(
                "::warning title={IDENTIFIER}::{}",
                format_github_message(&warning.message)
            ),
        }
    }
}

/// The move's own counters, reported under a `move` key rather than
/// mixed into `totals`, which stays a count of what went wrong.
#[derive(Serialize)]
struct MoveCounters<'a> {
    dry_run: bool,
    kind: &'a str,
    from: &'a str,
    to: &'a str,
    files_changed: usize,
    paths_moved: usize,
}

/// The whole `move` report; see [`json_body`].
#[derive(Serialize)]
struct MoveReport<'a> {
    totals: JsonTotals,
    files: BTreeMap<&'a str, JsonFileEntry<'a>>,
    errors: Vec<&'a str>,
    #[serde(rename = "move")]
    move_: MoveCounters<'a>,
}

/// Print the move as a JSON object shaped like `analyze`'s.
pub(super) fn print_json(summary: &MoveSummary) {
    println!("{}", json_body(summary));
}

/// Build the JSON document.
///
/// `totals` and `files` carry the same meaning they do in `analyze`'s
/// output, so the two can be consumed by the same tooling: warnings that
/// name a file are grouped under it with a line number, and the ones
/// that name none (an unmapped destination namespace, say) land in the
/// top-level `errors` array.
fn json_body(summary: &MoveSummary) -> String {
    let mut by_file: BTreeMap<&str, Vec<JsonMessage<'_>>> = BTreeMap::new();
    let mut global: Vec<&str> = Vec::new();
    for warning in &summary.warnings {
        match warning.file.as_deref() {
            Some(file) => by_file.entry(file).or_default().push(JsonMessage {
                message: &warning.message,
                line: warning.line.unwrap_or(1) as u64,
                severity: "warning",
                identifier: Some(IDENTIFIER),
            }),
            None => global.push(&warning.message),
        }
    }

    let report = MoveReport {
        totals: JsonTotals {
            errors: global.len(),
            file_errors: by_file.values().map(Vec::len).sum(),
        },
        files: by_file
            .into_iter()
            .map(|(file, messages)| {
                (
                    file,
                    JsonFileEntry {
                        errors: messages.len(),
                        messages,
                    },
                )
            })
            .collect(),
        errors: global,
        move_: MoveCounters {
            dry_run: summary.dry_run,
            kind: summary.kind,
            from: &summary.from,
            to: &summary.to,
            files_changed: summary.files_changed,
            paths_moved: summary.paths_moved,
        },
    };
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::move_cli::MoveWarning;

    fn summary(warnings: Vec<MoveWarning>) -> MoveSummary {
        MoveSummary {
            dry_run: true,
            kind: "namespace",
            from: "App\\Old".into(),
            to: "App\\New".into(),
            files_changed: 3,
            paths_moved: 1,
            warnings,
        }
    }

    #[test]
    fn json_is_valid_without_warnings() {
        let parsed: serde_json::Value =
            serde_json::from_str(&json_body(&summary(Vec::new()))).expect("valid json");
        assert_eq!(parsed["totals"]["errors"], 0);
        assert_eq!(parsed["totals"]["file_errors"], 0);
        assert_eq!(parsed["files"], serde_json::json!({}));
        assert_eq!(parsed["errors"], serde_json::json!([]));
        assert_eq!(parsed["move"]["paths_moved"], 1);
        assert_eq!(parsed["move"]["from"], "App\\Old");
    }

    #[test]
    fn json_groups_warnings_by_file() {
        let summary = summary(vec![
            MoveWarning {
                message: "still here".into(),
                file: Some("config/a.php".into()),
                line: Some(4),
            },
            MoveWarning {
                message: "also here".into(),
                file: Some("config/a.php".into()),
                line: Some(9),
            },
            MoveWarning {
                message: "no file".into(),
                file: None,
                line: None,
            },
        ]);
        let parsed: serde_json::Value =
            serde_json::from_str(&json_body(&summary)).expect("valid json");
        assert_eq!(parsed["totals"]["errors"], 1);
        assert_eq!(parsed["totals"]["file_errors"], 2);
        assert_eq!(parsed["files"]["config/a.php"]["errors"], 2);
        assert_eq!(parsed["files"]["config/a.php"]["messages"][1]["line"], 9);
        assert_eq!(
            parsed["files"]["config/a.php"]["messages"][0]["identifier"],
            IDENTIFIER
        );
        assert_eq!(parsed["errors"][0], "no file");
    }
}
