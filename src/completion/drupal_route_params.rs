//! Completion for the parameters a Drupal route's path declares.
//!
//! The parameters array of `Url::fromRoute('entity.node.canonical', ['|' =>
//! …])`, and of the other calls that take a route name and then its
//! parameters, is keyed by the placeholders of the named route's path,
//! which the route side tables keep beside each name (see
//! [`crate::symbol_map::DrupalRouteSite`]).  A static call or a `new Url()`
//! is recognised by its class, and a method call by its receiver, as its
//! route name is.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::source::code_context::{CodeContext, OpenBracket};
use crate::completion::symfony_string_keys::{is_instantiation, trailing_name};
use crate::symbol_map::StringKeyKind;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::FileContext;

impl Backend {
    /// Complete a key of the parameters array of a call that takes a Drupal
    /// route name.
    ///
    /// `None` when the cursor is not on such a key, or when the route's
    /// path declares no parameters.
    pub(crate) fn try_drupal_route_parameter_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        // A key starts the array or follows a comma in it.
        if !matches!(code.last_code_byte(), Some(b'[' | b',')) {
            return None;
        }
        // The array is an argument of the call, right inside it.
        let [.., call, array] = &code.open_brackets[..] else {
            return None;
        };
        if call.byte != b'(' || array.byte != b'[' {
            return None;
        }
        let route_index = self.drupal_route_argument(content, cursor, call, code, ctx)?;
        if call.commas != route_index + 1 {
            return None;
        }
        let route = string_argument(content, call.offset, route_index)?;

        let prefix = content.get(quote + 1..cursor)?.as_bytes();
        let edit_range = Range {
            start: offset_to_position(content, quote + 1),
            end: position,
        };
        let mut items: Vec<CompletionItem> = Vec::new();
        for path in self.drupal_route_paths(&route) {
            for name in crate::drupal::routing::path_parameter_names(&path) {
                let matches = name
                    .as_bytes()
                    .get(..prefix.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(prefix));
                if !matches || items.iter().any(|item| item.label == name) {
                    continue;
                }
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::Field),
                    detail: Some(path.clone()),
                    sort_text: Some(format!("{:05}", items.len())),
                    filter_text: Some(name.to_string()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: edit_range,
                        new_text: name.to_string(),
                    })),
                    ..Default::default()
                });
            }
        }
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }

    /// The argument the call whose `(` is `call` takes a Drupal route name
    /// in: `Url::fromRoute()`, `new Url()`, `Link::createFromRoute()`, or a
    /// method call whose receiver the type engine confirms takes one.
    fn drupal_route_argument(
        &self,
        content: &str,
        cursor: usize,
        call: &OpenBracket,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<usize> {
        let Some(operator) = &call.callee_operator else {
            let written = trailing_name(&content[..call.code_before]);
            let offset = call.code_before - written.len();
            let class = ctx.resolve_name_at(written, offset as u32);
            return (is_instantiation(&content[..offset]) && crate::drupal::is_url_class(&class))
                .then_some(0);
        };
        let found = detect_string_call_context(content, cursor, code)?;
        let subject = found.subject.as_deref()?;
        if operator.is_static {
            let class = ctx.resolve_name_at(subject, cursor as u32);
            return crate::drupal::static_route_argument(&class, &found.method_name);
        }
        crate::drupal::route_parameter_call(&found.method_name)?;
        (self.receiver_call_key(subject, &found.method_name, 0, content, cursor, ctx)
            == Some(StringKeyKind::DrupalRoute))
        .then_some(0)
    }
}

/// The string literal the call whose `(` is at `paren` passes as its
/// argument `index`, when the argument is one.
pub(crate) fn string_argument(content: &str, paren: usize, index: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut argument = 0;
    let mut depth = 0usize;
    // Whether the argument being read holds anything before a literal.
    let mut started = false;
    let mut at = paren + 1;
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'\'' | b'"') => {
                let mut end = at + 1;
                while end < bytes.len() && bytes[end] != quote {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                if depth == 0 && argument == index && !started {
                    return content.get(at + 1..end).map(str::to_string);
                }
                started = true;
                at = end + 1;
                continue;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                started = true;
            }
            b')' | b']' | b'}' => {
                depth = depth.checked_sub(1)?;
            }
            b',' if depth == 0 => {
                argument += 1;
                started = false;
                if argument > index {
                    return None;
                }
            }
            byte if !byte.is_ascii_whitespace() => started = true,
            _ => {}
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_argument_is_read_at_its_index() {
        let php = "Link::createFromRoute($this->t('Hi'), 'acme.greeting', ['";
        let paren = php.find('(').unwrap();
        assert_eq!(
            string_argument(php, paren, 1).as_deref(),
            Some("acme.greeting")
        );
        assert_eq!(string_argument(php, paren, 0), None);
        let php = "Url::fromRoute( 'it\\'s', [";
        assert_eq!(
            string_argument(php, php.find('(').unwrap(), 0).as_deref(),
            Some("it\\'s")
        );
    }
}
