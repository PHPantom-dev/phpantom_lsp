//! Completion for Drupal configuration names in PHP, where the string is
//! not a plain argument the Symfony completion reads: an element of the
//! list a config factory's `loadMultiple()` takes or a config form's
//! `getEditableConfigNames()` returns, and a form element's
//! `#config_target` before its `:`.
//!
//! `\Drupal::config('|')`, `new ConfigTarget('|')`, and the config
//! factory's and the `config()` shortcut's calls are read by the Symfony
//! completion, which confirms a method call's receiver.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::StringKeyKind;
use crate::text_position::position_to_offset;
use crate::types::FileContext;

impl Backend {
    /// Complete the configuration name the string at the cursor names.
    pub(crate) fn try_drupal_config_name_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let names = match code.last_code_byte()? {
            // `'#config_target' => '|'`: the name comes before the `:`.
            b'>' => {
                ends_with_key(code.code_before, "#config_target")
                    && !content[literal_start..cursor].contains(':')
            }
            b'[' | b',' => {
                let [.., inner] = &code.open_brackets[..] else {
                    return None;
                };
                inner.byte == b'['
                    && (returns_editable_names(content, inner.offset)
                        || self.is_config_list(content, cursor, code, ctx))
            }
            _ => false,
        };
        if !names {
            return None;
        }
        self.symfony_key_response(
            uri,
            StringKeyKind::DrupalConfigName,
            content,
            literal_start,
            cursor,
            position,
        )
    }

    /// Whether the array at the cursor is the list of names a config
    /// factory's `loadMultiple()` takes.
    fn is_config_list(
        &self,
        content: &str,
        cursor: usize,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> bool {
        let Some((_, paren)) = code.nested_pair(b'[', b'(') else {
            return false;
        };
        let Some(call) = detect_string_call_context(content, cursor, code) else {
            return false;
        };
        let Some(subject) = call.subject.as_deref() else {
            return false;
        };
        let opens_call = code
            .enclosing_paren()
            .is_some_and(|enclosing| enclosing.offset == paren);
        opens_call
            && !call.is_static
            && call.arg_index == 0
            && call.method_name.eq_ignore_ascii_case("loadMultiple")
            && self.receiver_call_key(subject, &call.method_name, 0, content, cursor, ctx)
                == Some(StringKeyKind::DrupalConfigName)
    }
}

/// Whether the code before the cursor ends with `'<key>' =>`.
fn ends_with_key(code_before: &str, key: &str) -> bool {
    let Some(before) = code_before.strip_suffix("=>") else {
        return false;
    };
    let before = before.trim_end();
    b"'\"".iter().any(|quote| {
        before
            .strip_suffix(*quote as char)
            .and_then(|rest| rest.strip_suffix(key))
            .is_some_and(|rest| rest.ends_with(*quote as char))
    })
}

/// Whether the array opened at `bracket` is returned by a method named
/// `getEditableConfigNames`: `return [` right inside it.
fn returns_editable_names(content: &str, bracket: usize) -> bool {
    let before = content[..bracket].trim_end();
    if !before.ends_with("return") {
        return false;
    }
    let Some(at) = before.rfind("function") else {
        return false;
    };
    let name = content[at + "function".len()..].trim_start();
    name.starts_with("getEditableConfigNames")
        && name["getEditableConfigNames".len()..]
            .trim_start()
            .starts_with('(')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_read_before_the_arrow() {
        assert!(ends_with_key("['#config_target' =>", "#config_target"));
        assert!(ends_with_key("[\"#config_target\"=>", "#config_target"));
        assert!(!ends_with_key("['x#config_target' =>", "#config_target"));
        assert!(!ends_with_key("['#title' =>", "#config_target"));
    }

    #[test]
    fn only_the_editable_names_method_returns_names() {
        let php = "protected function getEditableConfigNames(): array {\n    return ['";
        assert!(returns_editable_names(php, php.len() - 2));
        let php = "protected function other(): array {\n    return ['";
        assert!(!returns_editable_names(php, php.len() - 2));
        let php = "protected function getEditableConfigNames(): array {\n    $x = ['";
        assert!(!returns_editable_names(php, php.len() - 2));
    }
}
