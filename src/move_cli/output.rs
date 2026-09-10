//! Output formatting for the `move` command.
//!
//! The three formats match `analyze` and `fix` so a script driving a
//! batch of refactors reads all three the same way: a human-readable
//! summary, GitHub Actions workflow annotations, and a JSON object
//! shaped like the one `analyze` emits.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::analyse::{format_github_message, json_escape};

use super::{MoveSummary, MoveWarning};

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
        let message = format_github_message(&warning.message);
        match &warning.file {
            Some(file) => println!(
                "::warning file={file},line={line},col=0,title={IDENTIFIER}::{message}",
                line = warning.line.unwrap_or(1),
            ),
            None => println!("::warning title={IDENTIFIER}::{message}"),
        }
    }
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
/// top-level `errors` array. The move's own counters hang off a `move`
/// key rather than being mixed into `totals`, which stays a count of
/// what went wrong.
fn json_body(summary: &MoveSummary) -> String {
    let mut by_file: BTreeMap<&str, Vec<&MoveWarning>> = BTreeMap::new();
    let mut global: Vec<&MoveWarning> = Vec::new();
    for warning in &summary.warnings {
        match warning.file.as_deref() {
            Some(file) => by_file.entry(file).or_default().push(warning),
            None => global.push(warning),
        }
    }
    let file_warnings: usize = by_file.values().map(Vec::len).sum();

    let mut out = String::from("{\n");
    let _ = writeln!(
        out,
        "  \"totals\": {{ \"errors\": {}, \"file_errors\": {} }},",
        global.len(),
        file_warnings
    );

    if by_file.is_empty() {
        out.push_str("  \"files\": {},\n");
    } else {
        out.push_str("  \"files\": {\n");
        for (i, (file, warnings)) in by_file.iter().enumerate() {
            let _ = write!(
                out,
                "    {}: {{\n      \"errors\": {},\n      \"messages\": [\n",
                json_escape(file),
                warnings.len()
            );
            for (j, warning) in warnings.iter().enumerate() {
                let _ = write!(
                    out,
                    "        {{ \"message\": {}, \"line\": {}, \"severity\": \"warning\", \
                     \"identifier\": \"{IDENTIFIER}\" }}",
                    json_escape(&warning.message),
                    warning.line.unwrap_or(1),
                );
                if j + 1 < warnings.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("      ]\n    }");
            if i + 1 < by_file.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  },\n");
    }

    if global.is_empty() {
        out.push_str("  \"errors\": [],\n");
    } else {
        out.push_str("  \"errors\": [\n");
        for (i, warning) in global.iter().enumerate() {
            let _ = write!(out, "    {}", json_escape(&warning.message));
            if i + 1 < global.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
    }

    let _ = write!(
        out,
        "  \"move\": {{ \"dry_run\": {}, \"kind\": \"{}\", \"from\": {}, \"to\": {}, \
         \"files_changed\": {}, \"paths_moved\": {} }}\n}}",
        summary.dry_run,
        summary.kind,
        json_escape(&summary.from),
        json_escape(&summary.to),
        summary.files_changed,
        summary.paths_moved,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
