//! Completion of the values a parameter's `#[ExpectedValues]` attribute
//! declares.
//!
//! phpstorm-stubs mark the flag and mode parameters of many functions
//! with the constants they take, `pathinfo($path, |)` asking for one of
//! the `PATHINFO_*` constants and `phpinfo(INFO_GENERAL | |)` for more
//! `INFO_*` flags.  Inside such an argument the declared values are
//! offered ahead of the other names, and a flag parameter keeps offering
//! the flags not yet combined into it after each `|`.  A value that is a
//! string literal is also offered inside the quotes.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::StringCallContext;
use crate::completion::named_args::NamedArgContext;
use crate::types::{ExpectedValues, ExpectedValuesSource, FileContext, ParameterInfo, Visibility};

/// The argument the cursor is in, read from the text between the call's
/// `(` and the word being typed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CurrentArgument<'a> {
    /// The parameter a `name: value` argument names, without its `$`.
    pub name: Option<&'a str>,
    /// How many positional arguments come before it.
    pub index: usize,
    /// The argument's value up to the word being typed.
    pub value_before: &'a str,
}

/// Locate the argument being typed in `args_text`; see [`CurrentArgument`].
pub(crate) fn current_argument(args_text: &str) -> CurrentArgument<'_> {
    let (earlier, current) = match crate::text_scan::top_level_commas(args_text).last() {
        Some(comma) => (&args_text[..comma], &args_text[comma + 1..]),
        None => ("", args_text),
    };
    let index = crate::text_scan::split_top_level_commas(earlier)
        .iter()
        .filter(|arg| crate::completion::named_args::extract_named_arg_name(arg).is_none())
        .count();
    match split_named_argument(current) {
        Some((name, value_before)) => CurrentArgument {
            name: Some(name),
            index,
            value_before,
        },
        None => CurrentArgument {
            name: None,
            index,
            value_before: current,
        },
    }
}

/// Split `name: value` into the name and the value, or `None` for a
/// positional argument.
fn split_named_argument(arg: &str) -> Option<(&str, &str)> {
    let trimmed = arg.trim_start();
    let end = trimmed
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(trimmed.len());
    let (name, rest) = trimmed.split_at(end);
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let value = rest.trim_start().strip_prefix(':')?;
    if value.starts_with(':') {
        return None;
    }
    Some((name, value))
}

/// The parameter `arg` fills: the one it names, else the one at its
/// position, else a trailing variadic one.
pub(crate) fn target_parameter<'p>(
    parameters: &'p [ParameterInfo],
    arg: &CurrentArgument<'_>,
) -> Option<&'p ParameterInfo> {
    match arg.name {
        Some(name) => parameters
            .iter()
            .find(|p| p.name.strip_prefix('$') == Some(name)),
        None => parameters
            .get(arg.index)
            .or_else(|| parameters.last().filter(|p| p.is_variadic)),
    }
}

/// Whether `value` (as [`ExpectedValuesSource::Listed`] spells it) names a
/// constant rather than being a literal.
fn is_constant_name(value: &str) -> bool {
    value.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '\\')
        && !["true", "false", "null"]
            .iter()
            .any(|kw| value.eq_ignore_ascii_case(kw))
}

/// How the file described by `ctx` can name the class `fqn`: by its short
/// name when an import or the namespace brings it into scope, otherwise
/// fully qualified.
fn class_spelling(fqn: &str, ctx: &FileContext) -> String {
    let fqn = fqn.trim_start_matches('\\');
    let (namespace, short) = match fqn.rsplit_once('\\') {
        Some((ns, short)) => (Some(ns), short),
        None => (None, fqn),
    };
    let in_scope = match ctx
        .use_map
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(short))
    {
        Some((_, imported)) => imported.trim_start_matches('\\').eq_ignore_ascii_case(fqn),
        None => match (namespace, ctx.namespace.as_deref()) {
            (None, None) => true,
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        },
    };
    if in_scope {
        short.to_string()
    } else {
        format!("\\{fqn}")
    }
}

impl Backend {
    /// The values `expected` declares, each spelled the way the file
    /// described by `ctx` writes it.
    fn expected_value_texts(&self, expected: &ExpectedValues, ctx: &FileContext) -> Vec<String> {
        match &expected.source {
            ExpectedValuesSource::Listed(values) => values
                .iter()
                .map(
                    |value| match value.strip_prefix('\\').and_then(|v| v.split_once("::")) {
                        Some((class, constant)) => {
                            format!("{}::{constant}", class_spelling(class, ctx))
                        }
                        None => value.clone(),
                    },
                )
                .collect(),
            ExpectedValuesSource::FromClass(fqn) => {
                let class_loader = self.resolved_class_loader(ctx);
                let Some(class) = class_loader(crate::class_lookup::Fqn::new(fqn)) else {
                    return Vec::new();
                };
                let resolved = crate::virtual_members::resolve_class_fully_cached(
                    &class,
                    &class_loader,
                    &self.resolved_class_cache,
                );
                let spelling = class_spelling(fqn, ctx);
                resolved
                    .constants
                    .iter()
                    .filter(|c| c.visibility == Visibility::Public)
                    .map(|c| format!("{spelling}::{}", c.name))
                    .collect()
            }
        }
    }

    /// Completion items for the values the parameter under the cursor
    /// expects, when the cursor is where one can be written: at the start
    /// of the argument, or after a `|` in a flag argument.
    pub(crate) fn expected_value_items(
        &self,
        na_ctx: &NamedArgContext,
        parameters: &[ParameterInfo],
        ctx: &FileContext,
    ) -> Vec<CompletionItem> {
        let arg = current_argument(&na_ctx.args_text);
        let Some(expected) =
            target_parameter(parameters, &arg).and_then(|p| p.expected_values.as_deref())
        else {
            return Vec::new();
        };

        // A `\` the user typed before the word stays in the buffer, so
        // the inserted name must not repeat it.
        let mut before = arg.value_before.trim_end();
        let after_backslash = before.ends_with('\\');
        if after_backslash {
            before = before[..before.len() - 1].trim_end();
        }
        let combined: Vec<&str> = if before.is_empty() {
            Vec::new()
        } else if expected.is_flags && before.ends_with('|') && !before.ends_with("||") {
            before
                .split('|')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect()
        } else {
            return Vec::new();
        };

        let detail = if expected.is_flags {
            "Expected flag"
        } else {
            "Expected value"
        };
        self.expected_value_texts(expected, ctx)
            .into_iter()
            .filter(|value| !combined.contains(&value.as_str()))
            .filter(|value| !after_backslash || is_constant_name(value))
            .enumerate()
            .map(|(i, value)| {
                let bare = value.trim_start_matches('\\');
                let insert = if after_backslash {
                    bare
                } else {
                    value.as_str()
                };
                CompletionItem {
                    label: value.clone(),
                    kind: Some(if is_constant_name(&value) {
                        CompletionItemKind::Constant
                    } else {
                        CompletionItemKind::Value
                    }),
                    detail: Some(detail.to_string()),
                    insert_text: Some(insert.to_string()),
                    filter_text: Some(bare.to_string()),
                    // Ahead of the named arguments (`0_…`) and every name
                    // the other strategies offer.
                    sort_text: Some(format!("00_{i:03}")),
                    ..CompletionItem::default()
                }
            })
            .collect()
    }

    /// Completion of the string values the parameter a string argument
    /// fills expects, inside the quotes.
    pub(crate) fn expected_string_value_completion(
        &self,
        sc: &StringCallContext,
        param: &ParameterInfo,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let ExpectedValuesSource::Listed(values) = &param.expected_values.as_deref()?.source else {
            return None;
        };
        let partial = sc.partial.to_lowercase();
        let strings: Vec<String> = values
            .iter()
            .filter_map(|v| crate::util::unescape_php_string_literal(v))
            .filter(|s| s.to_lowercase().starts_with(&partial))
            .collect();
        crate::completion::laravel_string_keys::string_key_response(
            strings,
            CompletionItemKind::Value,
            content,
            sc.string_content_start,
            position,
        )
    }
}
