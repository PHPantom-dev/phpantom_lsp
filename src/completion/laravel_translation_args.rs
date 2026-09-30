//! Locale and replacement-key completion in translation calls.

use std::collections::BTreeSet;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::completion::source::code_context::{CodeContext, OpenBracket};
use crate::completion::source::helpers::{split_trailing_ident, trailing_class_name};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::FileContext;

struct TranslationCall {
    locale: usize,
    replace: Option<usize>,
}

fn translation_call(
    content: &str,
    paren: &OpenBracket,
    ctx: &FileContext,
) -> Option<TranslationCall> {
    let (name, _) = split_trailing_ident(&content[..paren.code_before]);
    let facade = if let Some(operator) = paren.callee_operator {
        if !operator.is_static {
            return None;
        }
        let receiver = trailing_class_name(&content[..operator.code_before]);
        let resolved =
            ctx.resolve_name_at(receiver, (operator.code_before - receiver.len()) as u32);
        if !resolved
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Lang")
            && !(receiver
                .trim_start_matches('\\')
                .eq_ignore_ascii_case("Lang")
                && !ctx.use_map.contains_key("Lang"))
        {
            return None;
        }
        true
    } else {
        let function = trailing_class_name(&content[..paren.code_before]);
        if function.trim_start_matches('\\').contains('\\') {
            return None;
        }
        false
    };
    match (facade, name.to_ascii_lowercase().as_str()) {
        (false, "__" | "trans") | (true, "get") => Some(TranslationCall {
            locale: 2,
            replace: Some(1),
        }),
        (false, "trans_choice") | (true, "choice") => Some(TranslationCall {
            locale: 3,
            replace: Some(2),
        }),
        (true, "has" | "hasforlocale") => Some(TranslationCall {
            locale: 1,
            replace: None,
        }),
        _ => None,
    }
}

#[derive(Default)]
struct Arguments {
    parameter: Option<usize>,
    key: Option<String>,
    used: BTreeSet<String>,
    string_end: usize,
}

struct ArgumentVisitor<'a> {
    paren: usize,
    quote: usize,
    call: &'a TranslationCall,
}

impl<'a> Walker<'a, 'a, Option<Arguments>> for ArgumentVisitor<'_> {
    fn walk_in_argument_list(&self, list: &'a ArgumentList<'a>, out: &mut Option<Arguments>) {
        if list.left_parenthesis.start.offset as usize != self.paren {
            return;
        }
        let mut result = Arguments::default();
        let mut positional = 0;
        for argument in list.arguments.iter() {
            let parameter = match argument {
                Argument::Positional(_) => {
                    let index = positional;
                    positional += 1;
                    Some(index)
                }
                Argument::Named(named) => match named.name.value {
                    b"key" => Some(0),
                    b"locale" => Some(self.call.locale),
                    b"replace" => self.call.replace,
                    _ => None,
                },
            };
            let value = argument.value();
            if parameter == Some(0) {
                result.key = literal(value).map(str::to_string);
            }
            let span = value.span();
            if span.start.offset as usize <= self.quote && self.quote < span.end.offset as usize {
                result.parameter = parameter;
                if let Expression::Literal(Literal::String(string)) = value {
                    result.string_end = string.span.end.offset as usize - 1;
                }
            }
            if parameter.is_some() && parameter == self.call.replace {
                let elements = match value {
                    Expression::Array(array) => array.elements.as_slice(),
                    _ => continue,
                };
                for element in elements {
                    let key = match element {
                        ArrayElement::KeyValue(entry) => entry.key,
                        ArrayElement::Value(entry) => entry.value,
                        _ => continue,
                    };
                    let span = key.span();
                    if span.start.offset as usize == self.quote {
                        result.string_end = span.end.offset as usize - 1;
                    } else if let Some(key) = literal(key) {
                        result.used.insert(key.to_string());
                    }
                }
            }
        }
        *out = Some(result);
    }
}

fn literal<'a>(value: &'a Expression<'_>) -> Option<&'a str> {
    if let Expression::Literal(Literal::String(string)) = value {
        string.value.map(bytes_to_str)
    } else {
        None
    }
}

fn arguments(
    content: &str,
    paren: usize,
    quote: usize,
    call: &TranslationCall,
) -> Option<Arguments> {
    crate::parser::with_parsed_program(content, "translation_arguments", |program, _| {
        let mut result = None;
        ArgumentVisitor { paren, quote, call }.walk_program(program, &mut result);
        result
    })
}

fn placeholders(value: &str, out: &mut BTreeSet<String>) {
    for (offset, _) in value.match_indices(':') {
        let rest = &value[offset + 1..];
        let length = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if length > 0 {
            out.insert(rest[..length].to_lowercase());
        }
    }
}

impl Backend {
    /// Complete a locale argument or the keys of a translation replacement array.
    pub(crate) fn try_translation_argument_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let (quote, quote_char) = code.open_string?;
        let paren = code.enclosing_paren()?;
        let call = translation_call(content, paren, ctx)?;
        let array = code.nested_pair(b'[', b'(').is_some();
        if array {
            if !matches!(code.last_code_byte(), Some(b'[' | b',')) {
                return None;
            }
        } else if code.open_brackets.last()?.offset != paren.offset
            || !matches!(code.last_code_byte(), Some(b'(' | b',' | b':'))
        {
            return None;
        }
        let cursor = position_to_offset(content, position) as usize;
        let args = arguments(content, paren.offset, quote, &call)
            .filter(|args| args.parameter.is_some())
            .or_else(|| {
                // Reuse a complete call even when surrounding syntax is broken.
                // Otherwise close only the prefix the cursor has reached.
                let close =
                    crate::text_scan::find_matching_forward(content, paren.offset, b'(', b')');
                let mut fragment = String::from("<?php f");
                if let Some(close) = close {
                    fragment.push_str(&content[paren.offset..=close]);
                } else {
                    fragment.push_str(&content[paren.offset..cursor]);
                    fragment.push(quote_char);
                    if array {
                        fragment.push(']');
                    }
                    fragment.push(')');
                }
                fragment.push(';');
                let mut args = arguments(&fragment, 7, quote - paren.offset + 7, &call)?;
                args.string_end = if close.is_some() {
                    args.string_end + paren.offset - 7
                } else {
                    cursor
                };
                Some(args)
            })?;
        let catalog = self.cached_translations();
        let (names, kind, detail) = if !array && args.parameter == Some(call.locale) {
            (
                catalog.locales.clone(),
                CompletionItemKind::VALUE,
                "Translation locale",
            )
        } else if array && args.parameter.is_some() && args.parameter == call.replace {
            let mut names = BTreeSet::new();
            for entry in catalog.entries.get(args.key.as_deref()?)? {
                if let Some(value) = &entry.value {
                    placeholders(value, &mut names);
                }
            }
            names.retain(|name| !args.used.contains(name));
            (names, CompletionItemKind::FIELD, "Translation placeholder")
        } else {
            return None;
        };
        let prefix = &content[quote + 1..cursor];
        let range = Range::new(
            offset_to_position(content, quote + 1),
            offset_to_position(content, args.string_end.max(cursor)),
        );
        Some(CompletionResponse::Array(
            names
                .into_iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| CompletionItem {
                    label: name.clone(),
                    kind: Some(kind),
                    detail: Some(detail.to_string()),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range,
                        new_text: name,
                    })),
                    ..Default::default()
                })
                .collect(),
        ))
    }
}

#[cfg(test)]
#[path = "laravel_translation_args_tests.rs"]
mod tests;
