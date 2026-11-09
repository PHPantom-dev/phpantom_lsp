//! Completion for Drupal configuration names in PHP, where the string is
//! not a plain argument the Symfony completion reads: an element of the
//! list a config factory's `loadMultiple()` takes or a config form's
//! `getEditableConfigNames()` returns, and a form element's
//! `#config_target` before its `:`.
//!
//! `\Drupal::config('|')`, `new ConfigTarget('|')`, and the config
//! factory's and the `config()` shortcut's calls are read by the Symfony
//! completion, which confirms a method call's receiver.
//!
//! A configuration key is completed one level at a time from the schema of
//! the object it belongs to (see `drupal::config_keys`): in a config
//! object's `get()`, `set()`, and `clear()`, whose receiver's type carries
//! the object's name (see `drupal::types`), after the `:` of a
//! `#config_target`, and in a `ConfigTarget`'s property path.  A `get()`
//! is a container lookup's too, so the Symfony completion reads it,
//! resolving its receiver once, and hands its keys to
//! [`Backend::drupal_config_key_response`].

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::laravel_string_keys::string_key_response;
use crate::completion::source::code_context::CodeContext;
use crate::completion::symfony_string_keys::{is_instantiation, trailing_name};
use crate::symbol_map::StringKeyKind;
use crate::text_position::position_to_offset;
use crate::types::{AccessKind, FileContext};

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
            // `'#config_target' => '|'`: the name comes before the `:`, and
            // the key after it.
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

    /// Complete the configuration key the string at the cursor names.
    pub(crate) fn try_drupal_config_key_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let typed = &content[literal_start..cursor];
        let (names, key_start) = match code.last_code_byte()? {
            // `'#config_target' => 'acme.settings:|'`.
            b'>' if ends_with_key(code.code_before, "#config_target") => {
                let (name, _) = typed.split_once(':')?;
                (vec![name.to_string()], literal_start + name.len() + 1)
            }
            b'(' | b',' | b'[' => (
                self.config_key_owners(content, cursor, code, ctx)?,
                literal_start,
            ),
            _ => return None,
        };
        self.drupal_config_key_response(&names, content, key_start, position)
    }

    /// Offer the keys of the configuration objects `names` at the level the
    /// key typed from `key_start` to the cursor has reached.
    pub(crate) fn drupal_config_key_response(
        &self,
        names: &[String],
        content: &str,
        key_start: usize,
        position: Position,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let path = content.get(key_start..cursor)?;
        let (parent, partial) = path.rsplit_once('.').unwrap_or(("", path));
        let mut keys: Vec<String> = names
            .iter()
            .flat_map(|name| self.drupal_config_keys(name, parent))
            .map(|key| key.name)
            .filter(|key| key.starts_with(partial))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        string_key_response(
            keys,
            CompletionItemKind::Field,
            content,
            cursor - partial.len(),
            position,
        )
    }

    /// The configuration objects whose keys the string at the cursor names,
    /// as an argument or an element of one: the names the receiver's type
    /// of a config object's `set()` or `clear()` carries, or the name a
    /// `new ConfigTarget()` takes before its property path.
    fn config_key_owners(
        &self,
        content: &str,
        cursor: usize,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<Vec<String>> {
        let call = detect_string_call_context(content, cursor, code)?;
        let paren = code.enclosing_paren()?;
        let in_list = match &code.open_brackets[..] {
            [.., outer, inner] if inner.byte == b'[' && outer.byte == b'(' => true,
            [.., inner] if inner.byte == b'(' => false,
            _ => return None,
        };
        let Some(subject) = call.subject.as_deref() else {
            let written = trailing_name(&content[..paren.code_before]);
            let offset = paren.code_before - written.len();
            let class = ctx.resolve_name_at(written, offset as u32);
            if !is_instantiation(&content[..offset])
                || !crate::drupal::is_config_target_class(&class)
                || call.arg_index != 1
            {
                return None;
            }
            let name =
                crate::completion::drupal_route_params::string_argument(content, paren.offset, 0)?;
            return Some(vec![name]);
        };
        // A `get()` is a container lookup's too, which the Symfony names
        // read, resolving its receiver once for both.
        let editable = call.method_name.eq_ignore_ascii_case("set")
            || call.method_name.eq_ignore_ascii_case("clear");
        if in_list || call.is_static || call.arg_index != 0 || !editable {
            return None;
        }
        let loaders = self.file_loaders(ctx);
        let rctx = loaders.resolution_ctx_at(content, cursor as u32);
        let mut names: Vec<String> = Vec::new();
        for resolved in
            crate::type_engine::resolver::resolve_target_classes(subject, AccessKind::Arrow, &rctx)
        {
            for name in self.drupal_config_names_of(
                &resolved.type_string,
                editable,
                loaders.resolved_class_loader(),
            ) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        (!names.is_empty()).then_some(names)
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
