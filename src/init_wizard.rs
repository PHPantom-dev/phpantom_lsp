//! Interactive prompts for `phpantom_lsp init`.
//!
//! Asks about the handful of settings most projects want to override at
//! setup time, and renders only the answers that differ from PHPantom's
//! defaults into a `.phpantom.toml` body. Settings that need a list or a
//! table (`indexing.exclude`, `[[diagnostics.ignore]]`, per-tool command
//! overrides) are left to manual editing, since a linear prompt sequence
//! cannot represent them well; the schema-aware header still points the
//! user at the docs for those.

use std::io::{self, Write};

use crate::config::CONFIG_HEADER;

/// Prompt the user over stdin/stderr and return the full
/// `.phpantom.toml` content to write.
pub fn run() -> String {
    let mut sections = Vec::new();

    let version = prompt_text("PHP version to target (blank = auto-detect from composer.json): ");
    if !version.is_empty() {
        sections.push(format!("[php]\nversion = \"{version}\"\n"));
    }

    let strategy = prompt_choice(
        "Indexing strategy: full (background-parse every file), composer (use Composer's classmap), self (scan every file, ignore Composer), none (on-demand only)",
        &["full", "composer", "self", "none"],
        "full",
    );
    if strategy != "full" {
        sections.push(format!("[indexing]\nstrategy = \"{strategy}\"\n"));
    }

    let mut diagnostics = Vec::new();
    if prompt_bool(
        "Compute diagnostics for the whole workspace in the background, not just open files?",
        false,
    ) {
        diagnostics.push("workspace = true");
    }
    if prompt_bool(
        "Report calls that pass more arguments than the function accepts?",
        false,
    ) {
        diagnostics.push("extra-arguments = true");
    }
    if prompt_bool(
        "Report member access on a subject whose type can't be resolved (mixed or unknown)?",
        false,
    ) {
        diagnostics.push("unresolved-member-access = true");
    }
    if !diagnostics.is_empty() {
        sections.push(format!("[diagnostics]\n{}\n", diagnostics.join("\n")));
    }

    let tokens_mode = prompt_choice(
        "Semantic token mode: contextual (defer ordinary syntax highlighting to the editor), full (emit every token), off",
        &["contextual", "full", "off"],
        "contextual",
    );
    if tokens_mode != "contextual" {
        sections.push(format!("[semantic_tokens]\nmode = \"{tokens_mode}\"\n"));
    }

    let mut content = CONFIG_HEADER.to_string();
    for section in sections {
        content.push('\n');
        content.push_str(&section);
    }
    content
}

fn prompt_text(question: &str) -> String {
    eprint!("{question}");
    let _ = io::stderr().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return String::new();
    }
    input.trim().to_string()
}

fn prompt_bool(question: &str, default: bool) -> bool {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let answer = prompt_text(&format!("{question} {hint} ")).to_lowercase();
        return match answer.as_str() {
            "" => default,
            "y" | "yes" => true,
            "n" | "no" => false,
            _ => {
                eprintln!("Please answer y or n.");
                continue;
            }
        };
    }
}

fn prompt_choice<'a>(question: &str, options: &[&'a str], default: &'a str) -> &'a str {
    loop {
        let answer = prompt_text(&format!("{question} [{default}]: "));
        if answer.is_empty() {
            return default;
        }
        if let Some(opt) = options.iter().find(|o| **o == answer) {
            return opt;
        }
        eprintln!("Please enter one of: {}", options.join(", "));
    }
}
