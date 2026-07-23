//! Completion for Artisan command parameters.
//!
//! Two related surfaces:
//!
//! - **Own arguments/options.** Inside a console command class,
//!   `$this->argument('|')` and `$this->option('|')` name segments of the
//!   *same* class's `$signature`.  The enclosing signature is parsed and its
//!   argument / option names offered.
//!
//! - **`Artisan::call` parameter arrays.** The second argument of
//!   `Artisan::call('app:sync', ['|' => ...])` (and `Artisan::queue`,
//!   `Schedule::command`, `$this->call`) is a map of the target command's
//!   arguments and `--options`, so the referenced command's parsed signature
//!   drives array-key completion.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::util::position_to_offset;

/// What kind of command-parameter completion the cursor sits in.
enum ParamContext {
    /// `$this->argument('|')` — complete this command's argument names.
    OwnArgument,
    /// `$this->option('|')` — complete this command's option names.
    OwnOption,
    /// A key string inside the parameter array of `Artisan::call('cmd', [ '|' ])`.
    CallArrayKey { command_name: String },
}

struct DetectedContext {
    context: ParamContext,
    prefix: String,
    /// Byte offset just after the opening quote of the string being typed.
    content_start_offset: usize,
}

impl Backend {
    /// Try completing an Artisan command parameter name.
    ///
    /// Returns `None` when the cursor is not inside a recognised
    /// command-parameter position.
    pub(crate) fn try_command_param_completion(
        &self,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let detected = detect_context(content, position)?;
        let cursor_offset = position_to_offset(content, position) as usize;

        let labels: Vec<String> = match &detected.context {
            ParamContext::OwnArgument => {
                let sig = crate::virtual_members::laravel::command_signature_at_offset(
                    content,
                    cursor_offset,
                )?;
                sig.arguments.into_iter().map(|p| p.name).collect()
            }
            ParamContext::OwnOption => {
                let sig = crate::virtual_members::laravel::command_signature_at_offset(
                    content,
                    cursor_offset,
                )?;
                sig.options.into_iter().map(|p| p.name).collect()
            }
            ParamContext::CallArrayKey { command_name } => {
                let index = self.laravel_commands.read();
                let entry = index.get(command_name)?;
                let mut labels: Vec<String> = entry
                    .signature
                    .arguments
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                labels.extend(
                    entry
                        .signature
                        .options
                        .iter()
                        .map(|o| format!("--{}", o.name)),
                );
                labels
            }
        };

        if labels.is_empty() {
            return None;
        }

        let start_pos = crate::util::offset_to_position(content, detected.content_start_offset);
        let edit_range = Range {
            start: start_pos,
            end: position,
        };
        let prefix_lower = detected.prefix.to_lowercase();

        let items: Vec<CompletionItem> = labels
            .into_iter()
            .filter(|name| {
                prefix_lower.is_empty() || name.to_lowercase().starts_with(&prefix_lower)
            })
            .enumerate()
            .map(|(i, name)| CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                sort_text: Some(format!("{:05}", i)),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: edit_range,
                    new_text: name,
                })),
                ..Default::default()
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }
}

/// Find the opening quote before the cursor and the prefix typed so far.
/// Returns `(quote_pos, prefix)` or `None` when the cursor is not inside an
/// unterminated single-line string.
fn find_open_quote(content: &str, cursor_offset: usize) -> Option<(usize, String)> {
    let bytes = content.as_bytes();
    if cursor_offset == 0 || cursor_offset > bytes.len() {
        return None;
    }
    let mut i = cursor_offset;
    while i > 0 {
        i -= 1;
        let ch = bytes[i];
        if ch == b'\'' || ch == b'"' {
            // Count preceding backslashes to skip escaped quotes.
            let mut bs = 0;
            let mut j = i;
            while j > 0 && bytes[j - 1] == b'\\' {
                bs += 1;
                j -= 1;
            }
            if bs % 2 == 0 {
                let prefix = content[i + 1..cursor_offset].to_string();
                return Some((i, prefix));
            }
        }
        if ch == b'\n' {
            return None;
        }
    }
    None
}

fn detect_context(content: &str, position: Position) -> Option<DetectedContext> {
    let cursor_offset = position_to_offset(content, position) as usize;
    let (quote_pos, prefix) = find_open_quote(content, cursor_offset)?;
    let before_quote = content[..quote_pos].trim_end();

    // ── Own argument / option: `->argument('|')` / `->option('|')` ─────────
    if let Some(before_paren) = before_quote.strip_suffix('(') {
        let before_paren = before_paren.trim_end();
        let (name, rest) = split_trailing_ident(before_paren);
        if !name.is_empty() {
            let is_method = rest.trim_end().ends_with("->") || rest.trim_end().ends_with("?->");
            if is_method {
                match name.to_ascii_lowercase().as_str() {
                    "argument" | "hasargument" | "getargument" => {
                        return Some(DetectedContext {
                            context: ParamContext::OwnArgument,
                            prefix,
                            content_start_offset: quote_pos + 1,
                        });
                    }
                    "option" | "hasoption" | "getoption" => {
                        return Some(DetectedContext {
                            context: ParamContext::OwnOption,
                            prefix,
                            content_start_offset: quote_pos + 1,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // ── Array key inside a command call's parameter array ──────────────────
    // e.g. `Artisan::call('app:sync', [ '|' => ... ])`.  The character before
    // the quote is `[` (first key) or `,` (subsequent key).
    let last = before_quote.chars().last()?;
    if (last == '[' || last == ',')
        && let Some(command_name) = command_name_for_array_key(content, quote_pos)
    {
        return Some(DetectedContext {
            context: ParamContext::CallArrayKey { command_name },
            prefix,
            content_start_offset: quote_pos + 1,
        });
    }

    None
}

/// Given the position of an array-key opening quote, resolve the command name
/// of the enclosing `Artisan::call('name', [...])`-style call.
///
/// Scans backwards for the `[` that opens the parameter array, then for the
/// preceding `(` that opens the call's argument list, extracts the first
/// string argument (the command name), and confirms the call is a recognised
/// command-running call.
fn command_name_for_array_key(content: &str, quote_pos: usize) -> Option<String> {
    let bytes = content.as_bytes();

    // Walk back to the `[` that opens the array, balancing nested brackets.
    let mut i = quote_pos;
    let mut depth = 0i32;
    let bracket_open = loop {
        if i == 0 {
            return None;
        }
        i -= 1;
        match bytes[i] {
            b']' => depth += 1,
            b'[' => {
                if depth == 0 {
                    break i;
                }
                depth -= 1;
            }
            b'\n' if depth == 0 => {
                // Allow the array to span lines; only bail on stray brackets.
            }
            _ => {}
        }
    };

    // Before the `[` we expect `... ('command', ` — find the `,` then the
    // preceding string literal (the command name) and the `(` and call name.
    let before_bracket = content[..bracket_open].trim_end();
    let before_bracket = before_bracket.strip_suffix(',')?.trim_end();

    // The command name is a trailing string literal.
    let (command_name, before_name) = trailing_string_literal(before_bracket)?;
    let before_name = before_name.trim_end();
    let before_paren = before_name.strip_suffix('(')?.trim_end();

    let (method, before_method) = split_trailing_ident(before_paren);
    let method = method.to_ascii_lowercase();
    let before_method = before_method.trim_end();

    let is_static = before_method.ends_with("::");
    let is_instance = before_method.ends_with("->") || before_method.ends_with("?->");

    let recognised = if is_static {
        let subject = trailing_class_name(&before_method[..before_method.len() - 2]);
        let subject = subject.rsplit('\\').next().unwrap_or(subject);
        matches!(
            (subject.to_ascii_lowercase().as_str(), method.as_str()),
            ("artisan", "call" | "queue") | ("schedule", "command")
        )
    } else if is_instance {
        matches!(method.as_str(), "call" | "callsilently")
    } else {
        false
    };

    recognised.then_some(command_name)
}

/// Split off a trailing PHP identifier (`[A-Za-z0-9_]+`) from `s`, returning
/// `(identifier, remainder_before_it)`.
fn split_trailing_ident(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    let mut start = bytes.len();
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    (&s[start..], &s[..start])
}

/// Split off a trailing class-name token (identifier plus `\` separators).
fn trailing_class_name(s: &str) -> &str {
    let s = s.trim_end();
    let bytes = s.as_bytes();
    let mut start = bytes.len();
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'_'
            || bytes[start - 1] == b'\\')
    {
        start -= 1;
    }
    &s[start..]
}

/// If `s` ends with a single- or double-quoted string literal, return its
/// inner value and the text before the opening quote.
fn trailing_string_literal(s: &str) -> Option<(String, &str)> {
    let s = s.trim_end();
    let bytes = s.as_bytes();
    let close = *bytes.last()?;
    if close != b'\'' && close != b'"' {
        return None;
    }
    // Find the matching opening quote (no escape handling needed for command
    // names, which never contain quotes).
    let mut i = bytes.len() - 1;
    while i > 0 {
        i -= 1;
        if bytes[i] == close {
            let value = s[i + 1..bytes.len() - 1].to_string();
            return Some((value, &s[..i]));
        }
    }
    None
}

#[cfg(test)]
#[path = "command_params_tests.rs"]
mod tests;
