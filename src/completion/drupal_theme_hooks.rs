//! Completion for the theme hooks a Drupal render array names.
//!
//! `'#theme' => '|'`, `$build['#theme'] = '|'`, and the names of a `#theme`
//! or `#theme_wrappers` list are completed from the theme hooks the site
//! declares (see [`crate::drupal::theme_hooks`]).  A key of an array whose
//! own `#theme` names a hook is completed from the hook's variables, as
//! `#name`.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::StringKeyKind;
use crate::text_position::{offset_to_position, position_to_offset};

/// The render array keys whose value names theme hooks.
const HOOK_KEYS: [&str; 2] = ["#theme", "#theme_wrappers"];

impl Backend {
    /// Complete a theme hook name, or a variable of the hook a render
    /// array's `#theme` names.  `None` when the cursor is on neither.
    pub(crate) fn try_drupal_theme_hook_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let cursor = position_to_offset(content, position) as usize;
        let prefix = content.get(quote + 1..cursor)?;
        let in_list = matches!(code.last_code_byte(), Some(b'[' | b','))
            && code
                .open_brackets
                .last()
                .is_some_and(|bracket| bracket.byte == b'[');
        let names_hook = names_theme_hook(code.code_before)
            || (in_list
                && code
                    .open_brackets
                    .last()
                    .is_some_and(|bracket| names_theme_hook(&content[..bracket.code_before])));
        if names_hook {
            return self.symfony_names_response(
                StringKeyKind::DrupalThemeHook,
                self.drupal_theme_hook_names(),
                content,
                quote + 1,
                cursor,
                position,
            );
        }
        if !in_list {
            return None;
        }
        let open = code.open_brackets.last()?.offset;
        let hook = array_theme_hook(content, open)?;
        let edit_range = Range {
            start: offset_to_position(content, quote + 1),
            end: position,
        };
        let items: Vec<CompletionItem> = self
            .drupal_theme_hook_variable_names(&hook)
            .into_iter()
            .filter(|name| starts_with_ignoring_case(name, prefix))
            .enumerate()
            .map(|(at, name)| CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::Field),
                detail: Some(format!("Theme hook `{hook}`")),
                sort_text: Some(format!("{at:05}")),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: name,
                })),
                ..Default::default()
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }
}

fn starts_with_ignoring_case(name: &str, prefix: &str) -> bool {
    name.as_bytes()
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Whether the code `before` a value ends with `'#theme' =>`, or
/// `['#theme'] =`, or the same with `#theme_wrappers`.
fn names_theme_hook(before: &str) -> bool {
    crate::drupal::elements::value_key(before).is_some_and(|key| HOOK_KEYS.contains(&key))
}

/// The hook the array literal opened at `open` names in a `'#theme' =>
/// 'name'` entry of its own, before or after the cursor.
fn array_theme_hook(content: &str, open: usize) -> Option<String> {
    enum Token<'a> {
        Literal(&'a str),
        Arrow,
        Other,
    }
    let bytes = content.as_bytes();
    let line_end =
        |from: usize| memchr::memchr(b'\n', &bytes[from..]).map_or(bytes.len(), |end| from + end);
    let mut depth = 0usize;
    let mut tokens: Vec<Token<'_>> = Vec::new();
    let mut at = open;
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'\'' | b'"') => {
                let mut end = at + 1;
                while end < bytes.len() && bytes[end] != quote {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                if depth == 1 {
                    tokens.push(Token::Literal(content.get(at + 1..end.min(bytes.len()))?));
                }
                at = end + 1;
                continue;
            }
            // A comment, which may hold a quote of its own.
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at = memchr::memmem::find(&bytes[at + 2..], b"*/")
                    .map_or(bytes.len(), |end| at + 2 + end + 2);
                continue;
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                at = line_end(at);
                continue;
            }
            b'#' if bytes.get(at + 1) != Some(&b'[') => {
                at = line_end(at);
                continue;
            }
            b'[' | b'(' | b'{' => {
                depth += 1;
                if depth > 1 {
                    tokens.push(Token::Other);
                }
            }
            b']' | b')' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            b'=' if depth == 1 && bytes.get(at + 1) == Some(&b'>') => {
                tokens.push(Token::Arrow);
                at += 2;
                continue;
            }
            b';' if depth == 1 => break,
            byte if depth == 1 && !byte.is_ascii_whitespace() => tokens.push(Token::Other),
            _ => {}
        }
        at += 1;
    }
    tokens
        .windows(4)
        .find_map(|window| match window {
            [
                Token::Literal("#theme"),
                Token::Arrow,
                Token::Literal(name),
                next,
            ] if !matches!(next, Token::Arrow) => Some(name.to_string()),
            _ => None,
        })
        .or_else(|| match tokens.as_slice() {
            [
                ..,
                Token::Literal("#theme"),
                Token::Arrow,
                Token::Literal(name),
            ] => Some(name.to_string()),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_after_a_theme_key_names_a_hook() {
        assert!(names_theme_hook("['#theme' => "));
        assert!(names_theme_hook("[\"#theme_wrappers\"=>"));
        assert!(names_theme_hook("$build['box']['#theme'] = "));
        assert!(!names_theme_hook("$build['#theme'] == "));
        assert!(!names_theme_hook("['#type' => "));
    }

    #[test]
    fn the_arrays_own_theme_entry_names_its_hook() {
        let php = "$x = ['#', '#theme' => 'acme_greeting', 'list' => ['#theme' => 'item_list']];";
        let open = php.find('[').unwrap();
        assert_eq!(
            array_theme_hook(php, open).as_deref(),
            Some("acme_greeting")
        );
        let php = "$x = ['#name' => 1, '#theme' => 'acme_greeting'];";
        assert_eq!(
            array_theme_hook(php, php.find('[').unwrap()).as_deref(),
            Some("acme_greeting")
        );
        let php = "$x = ['#theme' => ['a', 'b'], '#'];";
        assert_eq!(array_theme_hook(php, php.find('[').unwrap()), None);
    }

    #[test]
    fn a_quote_in_a_comment_does_not_hide_the_theme_entry() {
        for php in [
            "$x = [\n  // Don't cache.\n  '#theme' => 'acme_greeting',\n  '#na',\n];",
            "$x = [\n  # Don't cache.\n  '#theme' => 'acme_greeting',\n  '#na',\n];",
            "$x = [\n  /* Don't \"cache\". */\n  '#theme' => 'acme_greeting',\n  '#na',\n];",
            "$x = [\n  '#theme' => 'acme_greeting', // Don't cache.\n  '#na',\n];",
            "$x = [\n  '#theme' => 'acme_greeting',\n  #[Attribute] 'x' => fn() => 1,\n  '#na',\n];",
        ] {
            assert_eq!(
                array_theme_hook(php, php.find('[').unwrap()).as_deref(),
                Some("acme_greeting"),
                "{php}"
            );
        }
    }
}
