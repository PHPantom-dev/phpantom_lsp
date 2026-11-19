//! CSS class completion: the classes the project's stylesheets declare,
//! offered in a string given to a Yii HTML tag's `class()` or `addClass()`.
//!
//! The string holds several names separated by whitespace, so only the one
//! being typed is replaced, and the ones written before it are not offered
//! again.  The receiver is confirmed by the type engine, as the symbol map's
//! site for the string is (see `symbol_map/extraction/yii.rs`).

use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::laravel_string_keys::{string_key_item_kind, string_key_response};
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::{ReceiverClass, StringKeyKind};
use crate::text_position::position_to_offset;
use crate::types::FileContext;

impl Backend {
    /// Complete the class name a `class()` or `addClass()` string is being
    /// typed as.
    pub(crate) fn try_yii_css_class_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let call = detect_string_call_context(content, cursor, code)?;
        if call.is_static
            || !(call.method_name.eq_ignore_ascii_case("class")
                || call.method_name.eq_ignore_ascii_case("addClass"))
            // The names are the call's own arguments, never in an array.
            || !matches!(code.last_code_byte(), Some(b'(' | b','))
        {
            return None;
        }
        let typed = content.get(call.string_content_start..cursor)?;
        if typed.contains(['\\', '$']) {
            return None;
        }
        let subject = call.subject.as_deref()?;
        let kind = self.receiver_key_for(
            subject,
            (ReceiverClass::HtmlTag, &call.method_name, call.arg_index),
            content,
            cursor,
            ctx,
        )?;
        if kind != StringKeyKind::CssClass {
            return None;
        }
        let name_start = typed
            .rfind(|c: char| c.is_ascii_whitespace())
            .map_or(0, |space| space + 1);
        let written: Vec<&str> = crate::yii::css_class_names(&typed[..name_start])
            .map(|(_, name)| name)
            .collect();
        let names: Vec<String> = self
            .stylesheet_index()
            .names()
            .into_iter()
            .filter(|name| !written.contains(&name.as_str()))
            .collect();
        string_key_response(
            names,
            string_key_item_kind(&kind),
            content,
            call.string_content_start + name_start,
            position,
        )
    }
}
