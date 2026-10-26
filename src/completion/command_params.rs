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

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::text_position::position_to_offset;

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
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor_offset = position_to_offset(content, position) as usize;
        let detected = detect_context(content, cursor_offset, code)?;

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
                let index = self.laravel.commands.read();
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

        let start_pos =
            crate::text_position::offset_to_position(content, detected.content_start_offset);
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
                kind: Some(CompletionItemKind::Field),
                sort_text: Some(format!("{:05}", i)),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: name,
                })),
                ..Default::default()
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::CompletionItemList(items))
        }
    }
}

// `code_before` ends at the last byte of code before the opening quote of the
// string being typed, so a comment between the call and the key is skipped.
fn detect_context(
    content: &str,
    cursor_offset: usize,
    code: &CodeContext<'_>,
) -> Option<DetectedContext> {
    let (quote_pos, _) = code.open_string?;
    let prefix = content[quote_pos + 1..cursor_offset].to_string();
    let before_quote = code.code_before;

    // ── Own argument / option: `->argument('|')` / `->option('|')` ─────────
    if let Some(before_paren) = before_quote.strip_suffix('(') {
        let before_paren = before_paren.trim_end();
        let (name, rest) = crate::completion::source::helpers::split_trailing_ident(before_paren);
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
    let last = code.last_code_byte()?;
    if (last == b'[' || last == b',')
        && let Some(command_name) = command_name_for_array_key(content, code)
    {
        return Some(DetectedContext {
            context: ParamContext::CallArrayKey { command_name },
            prefix,
            content_start_offset: quote_pos + 1,
        });
    }

    None
}

/// Given the lexical context of an array-key opening quote, resolve the
/// command name of the enclosing `Artisan::call('name', [...])`-style call.
///
/// Returns `None` when the enclosing call is not a recognised
/// command-running call.
fn command_name_for_array_key(content: &str, code: &CodeContext<'_>) -> Option<String> {
    let call = crate::completion::source::helpers::enclosing_array_key_call(content, code)?;
    let before_callee = call.before_callee;

    let recognised = if let Some(receiver) = before_callee.strip_suffix("::") {
        let subject = crate::completion::source::helpers::trailing_class_name(receiver);
        let subject = subject.rsplit('\\').next().unwrap_or(subject);
        matches!(
            (subject.to_ascii_lowercase().as_str(), call.callee.as_str()),
            ("artisan", "call" | "queue") | ("schedule", "command")
        )
    } else if before_callee.ends_with("->") {
        matches!(call.callee.as_str(), "call" | "callsilently")
    } else {
        false
    };

    if !recognised {
        return None;
    }
    // The string may carry inline arguments (`'app:sync --limit=50'`); only
    // the first token is the command name.
    let name = call
        .name
        .split_whitespace()
        .next()
        .unwrap_or(&call.name)
        .to_string();
    Some(name)
}

#[cfg(test)]
#[path = "command_params_tests.rs"]
mod tests;
