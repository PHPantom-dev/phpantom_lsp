//! Yii alias completion: the aliases the project declares, offered in any
//! string whose text so far is an `@` and the start of a name.

use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::laravel_string_keys::{string_key_item_kind, string_key_response};
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::StringKeyKind;
use crate::text_position::position_to_offset;

impl Backend {
    /// Complete the alias a string's text starts with.
    pub(crate) fn try_yii_alias_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let typed = content.get(literal_start..cursor)?;
        if !typed.starts_with('@') || typed.contains('/') {
            return None;
        }
        string_key_response(
            self.yii_aliases().names(),
            string_key_item_kind(&StringKeyKind::YiiAlias),
            content,
            literal_start,
            position,
        )
    }
}
