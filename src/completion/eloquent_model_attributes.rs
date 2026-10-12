//! Column names in the PHP attributes attached to Eloquent model classes.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::FileContext;
use crate::virtual_members::laravel::extends_eloquent_model;
use crate::virtual_members::laravel::where_property::collect_column_names;

/// Attributes whose positional arguments are column names or an array of them.
const COLUMN_LIST_ATTRIBUTES: &[&str] = &[
    "Illuminate\\Database\\Eloquent\\Attributes\\Fillable",
    "Illuminate\\Database\\Eloquent\\Attributes\\Guarded",
    "Illuminate\\Database\\Eloquent\\Attributes\\Hidden",
    "Illuminate\\Database\\Eloquent\\Attributes\\Visible",
    "Illuminate\\Database\\Eloquent\\Attributes\\Appends",
];

impl Backend {
    /// Complete a column name in an installed Laravel model-level attribute.
    pub(crate) fn try_eloquent_model_attribute_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let cursor = position_to_offset(content, position) as usize;
        let (group, call) = code.open_brackets.windows(2).rev().find_map(|pair| {
            let group = &pair[0];
            let call = &pair[1];
            (group.byte == b'['
                && group.offset > 0
                && content.as_bytes()[group.offset - 1] == b'#'
                && call.byte == b'('
                && call.callee_operator.is_none())
            .then_some((group, call))
        })?;

        // The declaration span includes leading attributes, whereas the body
        // span would associate these arguments with no model at all. Requiring
        // the cursor before the keyword also excludes member attributes.
        let model = ctx.classes.iter().find(|class| {
            class.decl_start_offset <= group.offset as u32 && cursor < class.keyword_offset as usize
        })?;
        let before_call = content.get(..call.code_before)?;
        let name_start = before_call
            .bytes()
            .rposition(|byte| {
                !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\\' || byte >= 0x80)
            })
            .map_or(0, |offset| offset + 1);
        let raw_name = &before_call[name_start..];
        let name = if let Some(fqn) = raw_name.strip_prefix('\\') {
            fqn.to_string()
        } else {
            ctx.resolve_name_at(raw_name, name_start as u32)
        };
        let is_route_key =
            name.eq_ignore_ascii_case("Illuminate\\Database\\Eloquent\\Attributes\\RouteKey");
        if !is_route_key
            && !COLUMN_LIST_ATTRIBUTES
                .iter()
                .any(|attribute| name.eq_ignore_ascii_case(attribute))
        {
            return None;
        }

        let argument = string_argument(content, cursor, code, call)?;
        if is_route_key {
            if argument.position != StringArgumentPosition::Direct
                || !argument
                    .name
                    .as_deref()
                    .map_or(argument.index == 0, |name| name == "key")
            {
                return None;
            }
        } else {
            // Laravel reads the first variadic argument at numeric index zero;
            // named variadic arguments do not populate that index. An array is
            // unpacked only when it is the first argument.
            if argument.name.is_some()
                || (argument.index != 0 && argument.first_is_array)
                || !matches!(
                    argument.position,
                    StringArgumentPosition::Direct
                        | StringArgumentPosition::ArrayElement
                        | StringArgumentPosition::ArrayValue
                )
                || (argument.position != StringArgumentPosition::Direct && argument.index != 0)
            {
                return None;
            }
        }

        let class_loader = self.class_loader_with(
            &ctx.classes,
            ctx.use_map_at(name_start as u32),
            ctx.namespace_at(name_start as u32),
        );
        // Recognition alone is not a version check: older Laravel installs
        // may not provide the requested attribute class at all.
        let attribute = class_loader(&format!("\\{name}"))?;
        if !attribute.fqn().eq_ignore_ascii_case(&name) {
            return None;
        }
        if !extends_eloquent_model(model, &class_loader) {
            return None;
        }
        let model = crate::virtual_members::resolve_class_fully_cached(
            model,
            &class_loader,
            &self.resolved_class_cache,
        );
        let prefix = content.get(quote + 1..cursor)?.to_lowercase();
        let range = Range {
            start: offset_to_position(content, quote + 1),
            end: offset_to_position(content, argument.string_end),
        };
        let items: Vec<_> = collect_column_names(&model)
            .into_iter()
            .filter(|column| {
                column.to_lowercase().starts_with(&prefix) && !argument.used_values.contains(column)
            })
            .map(|column| CompletionItem {
                label: column.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("column".to_string()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: column,
                })),
                ..Default::default()
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::Array(items))
    }
}
