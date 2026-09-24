//! Add a missing translation to an existing PHP language group.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::symbol_map::{LaravelStringKind, SymbolKind};
use crate::text_position::{offset_to_position, ranges_overlap};

impl Backend {
    /// Offer one insertion per existing locale file for an unknown translation.
    pub(crate) fn collect_insert_translation_key_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let diagnostics: Vec<_> = params.context.diagnostics.iter().filter(|diagnostic| {
            matches!(&diagnostic.code, Some(NumberOrString::String(code)) if code == "invalid_laravel_trans")
                && ranges_overlap(&diagnostic.range, &params.range)
        }).collect();
        if diagnostics.is_empty() {
            return;
        }
        let Some(symbol_map) = self.symbol_maps.read().get(uri).cloned() else {
            return;
        };
        let catalog = self.cached_translations();
        for span in &symbol_map.spans {
            let SymbolKind::LaravelStringKey {
                kind: LaravelStringKind::Trans,
                key,
                is_write: false,
                ..
            } = &span.kind
            else {
                continue;
            };
            let range = Range::new(
                offset_to_position(content, span.start as usize),
                offset_to_position(content, span.end as usize),
            );
            let Some(diagnostic) = diagnostics
                .iter()
                .find(|diagnostic| ranges_overlap(&range, &diagnostic.range))
            else {
                continue;
            };
            if catalog.entries.contains_key(key) {
                continue;
            }
            let Some((group, path)) = key.split_once('.') else {
                continue;
            };
            if path.split('.').any(str::is_empty) {
                continue;
            }
            for file in catalog
                .files
                .iter()
                .filter(|file| file.group.as_deref() == Some(group))
            {
                let Some(source) = self.get_file_content(file.uri.as_str()) else {
                    continue;
                };
                let Some(edits) = insertion_edits(&source, path) else {
                    continue;
                };
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Insert translation '{}' ({})", key, file.locale),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![(*diagnostic).clone()]),
                    edit: Some(super::helpers::single_file_edit(file.uri.clone(), edits)),
                    ..Default::default()
                }));
            }
        }
    }
}

fn insertion_edits(content: &str, path: &str) -> Option<Vec<TextEdit>> {
    crate::parser::with_parsed_program(content, "insert_translation_key", |program, _| {
        if !program.errors.is_empty() {
            return None;
        }
        let returned = program
            .statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Return(ret) => ret.value,
                _ => None,
            })?;
        insert_into_array(content, returned, &path.split('.').collect::<Vec<_>>())
    })
}

fn insert_into_array(
    content: &str,
    expression: &Expression<'_>,
    path: &[&str],
) -> Option<Vec<TextEdit>> {
    let (elements, open, close) = match expression {
        Expression::Array(array) => (
            &array.elements,
            array.left_bracket.end.offset as usize,
            array.right_bracket.start.offset as usize,
        ),
        Expression::LegacyArray(array) => (
            &array.elements,
            array.left_parenthesis.end.offset as usize,
            array.right_parenthesis.start.offset as usize,
        ),
        Expression::Parenthesized(parenthesized) => {
            return insert_into_array(content, parenthesized.expression, path);
        }
        _ => return None,
    };
    for element in elements.iter().rev() {
        let ArrayElement::KeyValue(entry) = element else {
            return None;
        };
        let Expression::Literal(Literal::String(key)) = entry.key else {
            return None;
        };
        if key.value.map(bytes_to_str)? == path[0] {
            return if path.len() > 1 {
                insert_into_array(content, entry.value, &path[1..])
            } else {
                None
            };
        }
    }
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let multiline = content[open..close].contains('\n');
    let close_line = content[..close].rfind('\n').map_or(0, |offset| offset + 1);
    let close_indent = &content[close_line..close];
    let own_line = close_indent
        .bytes()
        .all(|byte| byte == b' ' || byte == b'\t');
    let indent = elements
        .first()
        .map(|element| indentation(content, element.span().start.offset as usize))
        .filter(|indent| !indent.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}    ", indentation(content, close)));
    let entry = nested_entry(path);
    let mut edits = Vec::new();
    let last_end = elements
        .last()
        .map(|element| element.span().end.offset as usize);
    if let Some(last_end) = last_end
        && !elements.has_trailing_token()
    {
        edits.push(TextEdit {
            range: Range::new(
                offset_to_position(content, last_end),
                offset_to_position(content, last_end),
            ),
            new_text: ",".to_string(),
        });
    }
    let (offset, text) = if multiline && own_line {
        (close_line, format!("{indent}{entry},{newline}"))
    } else if multiline {
        (
            close,
            format!(
                "{newline}{indent}{entry},{newline}{}",
                indentation(content, open)
            ),
        )
    } else {
        (
            close,
            format!(
                "{}{entry}{}",
                if last_end.is_some() { " " } else { "" },
                if elements.has_trailing_token() {
                    ","
                } else {
                    ""
                }
            ),
        )
    };
    edits.push(TextEdit {
        range: Range::new(
            offset_to_position(content, offset),
            offset_to_position(content, offset),
        ),
        new_text: text,
    });
    Some(edits)
}

fn indentation(content: &str, offset: usize) -> &str {
    let start = content[..offset].rfind('\n').map_or(0, |index| index + 1);
    let line = &content[start..offset];
    &line[..line
        .bytes()
        .take_while(|byte| *byte == b' ' || *byte == b'\t')
        .count()]
}

fn nested_entry(path: &[&str]) -> String {
    let mut entry = String::new();
    for (index, key) in path.iter().enumerate() {
        if index > 0 {
            entry.push('[');
        }
        entry.push('\'');
        entry.push_str(&key.replace('\\', "\\\\").replace('\'', "\\'"));
        entry.push_str("' => ");
    }
    entry.push_str("''");
    for _ in 1..path.len() {
        entry.push(']');
    }
    entry
}

#[cfg(test)]
#[path = "insert_translation_key_tests.rs"]
mod tests;
