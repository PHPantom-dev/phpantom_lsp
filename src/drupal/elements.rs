//! Drupal's render elements: the names a `#[RenderElement]` or
//! `#[FormElement]` class (or the annotation older code uses) declares,
//! read by the `#type` of a render array.

use tower_lsp_server::gen_lsp_types::{CompletionResponse, Location, Position};

use crate::Backend;
use crate::atom::Atom;
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::{StringKeyKind, SymbolKind};

/// The `#`-prefixed key of a render array whose value is the string that
/// starts after `code_before`, the code before its opening quote:
/// `'#type' => '…'` in an array literal, or `$a['#type'] = '…'`.
pub(crate) fn render_array_value_key(code_before: &str) -> Option<&str> {
    value_key(code_before).filter(|key| key.starts_with('#'))
}

/// The literal key whose value starts after `code_before`: the key of a
/// `'key' =>` entry, or the last offset of a `$a['key'] =` assignment.
pub(crate) fn value_key(code_before: &str) -> Option<&str> {
    arrow_key(code_before).or_else(|| {
        let target = assignment_target(code_before)?;
        trailing_literal(target.trim_end().strip_suffix(']')?)
    })
}

/// The literal key of the `'key' =>` entry `before` ends with.
pub(crate) fn arrow_key(before: &str) -> Option<&str> {
    trailing_literal(before.trim_end().strip_suffix("=>")?)
}

/// The code `before`, which ends with an assignment's `=`, assigns to: the
/// code before a plain `=`, not a comparison or a compound assignment.
pub(crate) fn assignment_target(before: &str) -> Option<&str> {
    let target = before.trim_end().strip_suffix('=')?;
    (!target.ends_with([
        '=', '!', '<', '>', '.', '+', '-', '*', '/', '%', '&', '|', '^', '?', ':',
    ]))
    .then_some(target)
}

/// The contents of the string literal `text` ends with.
pub(crate) fn trailing_literal(text: &str) -> Option<&str> {
    let text = text.trim_end();
    let quote = text
        .chars()
        .next_back()
        .filter(|c| matches!(c, '\'' | '"'))?;
    let inner = &text[..text.len() - 1];
    Some(&inner[inner.rfind(quote)? + 1..])
}

/// Whether `target` ends with the offsets `['keys…']`, in order.
pub(crate) fn ends_with_keys(mut target: &str, keys: &[&str]) -> bool {
    for key in keys.iter().rev() {
        let Some(rest) = target
            .trim_end()
            .strip_suffix(']')
            .and_then(|rest| strip_quoted(rest.trim_end(), key))
            .and_then(|rest| rest.trim_end().strip_suffix('['))
        else {
            return false;
        };
        target = rest;
    }
    true
}

/// `text` without the string literal `key` it ends with.
fn strip_quoted<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    ['\'', '"'].into_iter().find_map(|quote| {
        text.strip_suffix(quote)?
            .strip_suffix(key)?
            .strip_suffix(quote)
    })
}

impl Backend {
    /// The classes that declare the plugin `id` as a key of `kind`, in a
    /// stable order.
    pub(crate) fn drupal_plugin_classes(&self, kind: StringKeyKind, id: &str) -> Vec<Atom> {
        let mut found: Vec<Atom> = Vec::new();
        self.visit_declaring_symbol_maps(|_, map| {
            for site in &map.drupal_plugin_classes {
                let at = map.spans.partition_point(|span| span.start < site.start);
                let declares_id = map.spans[at..]
                    .iter()
                    .take_while(|span| span.start == site.start)
                    .any(|span| {
                        matches!(
                            &span.kind,
                            SymbolKind::StringKey { kind: declared, key, .. }
                                if *declared == kind && key == id
                        )
                    });
                if declares_id && !found.contains(&site.class) {
                    found.push(site.class);
                }
            }
        });
        found
    }

    /// Where the render element `name` is declared: the name of each class
    /// that declares it, else the ID its declaration writes.
    pub(crate) fn drupal_render_element_definitions(&self, name: &str) -> Vec<Location> {
        let kind = StringKeyKind::DrupalRenderElement;
        let classes: Vec<Location> = self
            .drupal_plugin_classes(kind, name)
            .iter()
            .filter_map(|class| self.class_declaration_location(class))
            .collect();
        if classes.is_empty() {
            self.symfony_declaration_locations(kind, name)
        } else {
            classes
        }
    }

    /// The hover detail of the render element `name`: the classes that
    /// declare it and the file of the first.
    pub(crate) fn drupal_render_element_detail(&self, name: &str) -> String {
        let kind = StringKeyKind::DrupalRenderElement;
        let classes = self.drupal_plugin_classes(kind, name);
        if classes.is_empty() {
            return "Not declared by any element class in the project".to_string();
        }
        let mut detail = classes
            .iter()
            .map(|class| format!("Class `{class}`"))
            .collect::<Vec<_>>()
            .join("\n\n");
        if let Some(path) = self
            .symfony_declaration_locations(kind, name)
            .first()
            .and_then(|location| self.workspace_relative_path(location.uri.as_str()))
        {
            detail.push_str(&format!("\n\nDeclared in `{path}`"));
        }
        detail
    }

    /// Complete a render element name in the `#type` of a render array.
    pub(crate) fn try_drupal_render_element_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        if render_array_value_key(code.code_before)? != "#type" {
            return None;
        }
        let cursor = crate::text_position::position_to_offset(content, position) as usize;
        self.symfony_key_response(
            uri,
            StringKeyKind::DrupalRenderElement,
            content,
            quote + 1,
            cursor,
            position,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::render_array_value_key;

    #[test]
    fn the_key_before_a_value_is_read_from_entries_and_assignments() {
        assert_eq!(render_array_value_key("return ['#type' =>"), Some("#type"));
        assert_eq!(render_array_value_key("[\"#type\"=>"), Some("#type"));
        assert_eq!(
            render_array_value_key("$form['name']['#type'] ="),
            Some("#type")
        );
        assert_eq!(
            render_array_value_key("$form['name'][ \"#type\" ]="),
            Some("#type")
        );
        assert_eq!(render_array_value_key("['type' =>"), None);
        assert_eq!(render_array_value_key("$form['#type'] =="), None);
        assert_eq!(render_array_value_key("$form['#type'] .="), None);
        assert_eq!(render_array_value_key("$form['#type'] !="), None);
        assert_eq!(render_array_value_key("$form['#type'] ??="), None);
        assert_eq!(render_array_value_key("$type ="), None);
        assert_eq!(render_array_value_key("foo("), None);
    }
}
