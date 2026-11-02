//! Completion of Drupal hook implementations: typing a function name in an
//! extension's procedural file offers `<extension>_<hook>` for every
//! documented hook, with the documented parameters.

use tower_lsp_server::gen_lsp_types::{
    CompletionItem, CompletionItemKind, InsertTextFormat, Position, Range, TextEdit,
};

use crate::Backend;
use crate::drupal::hooks::DrupalFile;
use crate::types::FileContext;

/// The function name being typed after `function ` at `cursor`, when the
/// line holds nothing else: what is typed so far, and where it starts.
fn function_name_at(content: &str, cursor: usize) -> Option<(&str, usize)> {
    let before = content.get(..cursor)?;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let line = &before[line_start..];
    let after_keyword = line.trim_start().strip_prefix("function")?;
    let typed = after_keyword.trim_start();
    let starts_with_space = after_keyword.len() > typed.len();
    let is_name = typed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    (starts_with_space && is_name).then(|| (typed, cursor - typed.len()))
}

/// Whether the line that starts at `line_start` starts a statement, rather
/// than continuing an expression: the last line before it that is not
/// blank, a comment, or an attribute ends a statement or opens or closes a
/// block, or there is none but the opening tag.  A closure written on a
/// line of its own inside a call's arguments is not a declaration.
fn starts_statement(content: &str, line_start: usize) -> bool {
    for line in content[..line_start].lines().rev() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with('#')
            || line.starts_with('*')
            || line.starts_with("/*")
            || line.ends_with("*/")
        {
            continue;
        }
        return line.starts_with("<?php") || line.ends_with([';', '{', '}']);
    }
    true
}

/// Escape `text` for a snippet, where `$`, `}`, and `\` mean something.
fn snippet_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '$' | '}' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The hook name as a snippet: each placeholder of a documented pattern
/// (`FORM_ID`) is a tab stop holding its name.
fn hook_snippet(hook: &str) -> (String, usize) {
    let mut snippet = String::new();
    let mut stops = 0;
    for piece in crate::drupal::hooks::pieces(hook) {
        match piece {
            crate::drupal::hooks::Piece::Text(text) => snippet.push_str(text),
            crate::drupal::hooks::Piece::Placeholder(name) => {
                stops += 1;
                snippet.push_str(&format!("${{{stops}:{name}}}"));
            }
        }
    }
    (snippet, stops)
}

impl Backend {
    /// The hook implementations a function name typed at `cursor` in an
    /// extension's procedural file can be: only for a function declared at
    /// the top level, not a method, a nested function, or a closure, and
    /// only when one of them matches, so that every other completion still
    /// answers there.
    pub(crate) fn drupal_hook_function_completion(
        &self,
        uri: &str,
        content: &str,
        cursor: usize,
        position: Position,
        ctx: &FileContext,
    ) -> Option<Vec<CompletionItem>> {
        let DrupalFile::Procedural(procedural) = self.drupal_file(uri) else {
            return None;
        };
        let (typed, start) = function_name_at(content, cursor)?;
        let line_start = content[..cursor].rfind('\n').map_or(0, |index| index + 1);
        let offset = u32::try_from(cursor).unwrap_or(u32::MAX);
        let in_body = crate::class_lookup::find_class_at_offset(&ctx.classes, offset).is_some()
            || self
                .symbol_map_for(uri)
                .is_some_and(|map| map.find_enclosing_scope(offset) != 0);
        if in_body || !starts_statement(content, line_start) {
            return None;
        }
        let start_position = crate::text_position::offset_to_position(content, start);
        let mut items = Vec::new();
        for hook in self.drupal_documented_hooks() {
            let name = format!("{}_{hook}", procedural.extension);
            let matches_typed = name
                .as_bytes()
                .get(..typed.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(typed.as_bytes()));
            if !matches_typed {
                continue;
            }
            let doc = format!("hook_{hook}");
            let parameters = self
                .find_or_load_function(&[doc.as_str()])
                .map(|function| self.hook_parameters(&function.parameters, ctx))
                .unwrap_or_default();
            let (hook_part, stops) = hook_snippet(&hook);
            let snippet = format!(
                "{}_{hook_part}({}) {{\n\t${}\n}}",
                snippet_text(&procedural.extension),
                snippet_text(&parameters),
                stops + 1
            );
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::Function),
                detail: Some(format!("Implements {doc}()")),
                filter_text: Some(name),
                insert_text_format: Some(InsertTextFormat::Snippet),
                text_edit: Some(
                    tower_lsp_server::gen_lsp_types::CompletionItemTextEdit::TextEdit(TextEdit {
                        range: Range {
                            start: start_position,
                            end: position,
                        },
                        new_text: snippet,
                    }),
                ),
                ..CompletionItem::default()
            });
        }
        (!items.is_empty()).then_some(items)
    }

    /// The documented parameters, as an implementation writes them: the
    /// native hints, shortened by the file's imports.
    fn hook_parameters(
        &self,
        parameters: &[crate::types::ParameterInfo],
        ctx: &FileContext,
    ) -> String {
        parameters
            .iter()
            .map(|parameter| {
                let hint = parameter
                    .native_type_hint
                    .as_ref()
                    .map(|hint| {
                        hint.resolve_names(&|name| {
                            crate::names::shorten_from_fqn(name, &ctx.use_map, &ctx.namespace)
                        })
                        .to_string()
                    })
                    .map(|hint| format!("{hint} "))
                    .unwrap_or_default();
                let sigil = if parameter.is_variadic {
                    "..."
                } else if parameter.is_reference {
                    "&"
                } else {
                    ""
                };
                let default = parameter
                    .default_value
                    .as_ref()
                    .filter(|_| !parameter.is_required && !parameter.is_variadic)
                    .map(|value| format!(" = {value}"))
                    .unwrap_or_default();
                format!("{hint}{sigil}{}{default}", parameter.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_function_name_after_the_keyword_is_completed() {
        let php = "<?php\nfunction acme_fo";
        assert_eq!(
            function_name_at(php, php.len()),
            Some(("acme_fo", php.len() - 7))
        );
        let php = "<?php\nfunction ";
        assert_eq!(function_name_at(php, php.len()), Some(("", php.len())));
        assert_eq!(function_name_at("<?php\nfunctionx", 15), None);
        assert_eq!(function_name_at("<?php\n$x = function_", 19), None);
    }

    #[test]
    fn placeholders_are_tab_stops() {
        assert_eq!(
            hook_snippet("form_FORM_ID_alter"),
            ("form_${1:FORM_ID}_alter".to_string(), 1)
        );
        assert_eq!(hook_snippet("cron"), ("cron".to_string(), 0));
        assert_eq!(snippet_text("&$form"), "&\\$form");
    }
}
