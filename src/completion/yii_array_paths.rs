//! Completion and go-to-definition in the key paths `ArrayHelper` reads.
//!
//! In `ArrayHelper::getValueByPath($user, 'profile.|')`, and in the keys
//! of a list (`getValue($user, ['profile', '|'])`), each segment completes
//! from the keys and the properties of the value the segments before it
//! reach, walked by the shared key-path walk
//! ([`crate::type_engine::key_path`]) from the type of the call's first
//! argument.  A segment that reads a property or calls a method jumps to
//! it.  Which calls take a path, and how they split it, is
//! [`crate::yii::array_helper`]'s.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::extract_identifier_backwards;
use crate::completion::source::code_context::{CodeContext, closing_call_paren, code_context_at};
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::php_type::PhpType;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::type_engine::key_path::{self, SegmentKind};
use crate::type_engine::resolver::resolve_target_classes;
use crate::types::{AccessKind, FileContext, ResolvedType};
use crate::yii::array_helper::{PathMethod, delimiter, path_keys, path_method, split_path};

/// The segment of a key path a string argument holds at an offset.
struct PathSite<'c> {
    method: &'static PathMethod,
    /// The value the path is read from, as written.
    value: &'c str,
    /// The keys before the segment.
    keys: Vec<String>,
    /// Where the segment starts.
    start: usize,
    /// Where the segment ends: at the next delimiter, or the string's end.
    end: usize,
}

impl Backend {
    /// Complete a segment of the key path an `ArrayHelper` call reads.
    /// `None` when the cursor is in no such path.
    pub(crate) fn try_yii_array_path_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let site = path_site(content, cursor, ctx, code)?;
        let prefix = content.get(site.start..cursor)?;
        let loaders = self.file_loaders(ctx);
        let rctx = loaders.resolution_ctx_at(content, cursor as u32);
        let reached = reached_type(&site, &rctx)?;
        let (quote, _) = code.open_string?;
        let quote = content[quote..].chars().next()?;
        let edit_range = Range {
            start: offset_to_position(content, site.start),
            end: position,
        };
        let items: Vec<CompletionItem> =
            key_path::segment_names(&reached, site.method.rules(), &rctx)
                .into_iter()
                .filter(|segment| {
                    // A name the path would split, or the literal end, is
                    // left out rather than escaped.
                    !segment.name.contains(['\\', quote])
                        && !(site.method.split && segment.name.contains('.'))
                        && segment
                            .name
                            .as_bytes()
                            .get(..prefix.len())
                            .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
                })
                .enumerate()
                .map(|(at, segment)| CompletionItem {
                    label: segment.name.clone(),
                    kind: Some(match segment.kind {
                        SegmentKind::Key => CompletionItemKind::Field,
                        SegmentKind::Property => CompletionItemKind::Property,
                        SegmentKind::Method => CompletionItemKind::Method,
                    }),
                    detail: segment.ty.as_ref().map(PhpType::to_string),
                    sort_text: Some(format!("{at:05}")),
                    filter_text: Some(segment.name.clone()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: edit_range,
                        new_text: segment.name,
                    })),
                    ..Default::default()
                })
                .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }

    /// Go to the property or the method a segment of the key path an
    /// `ArrayHelper` call reads names.
    pub(crate) fn yii_array_path_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Location> {
        // Spare the forward scan in a file that never names the class: an
        // import alias still spells it in the `use`.
        if !content.contains("ArrayHelper") {
            return None;
        }
        let offset = position_to_offset(content, position) as usize;
        let code = code_context_at(content, offset)?;
        code.open_string?;
        let ctx = self.file_context(uri);
        let site = path_site(content, offset, &ctx, &code)?;
        let name = content.get(site.start..site.end)?;
        if name.is_empty() {
            return None;
        }
        let loaders = self.file_loaders(&ctx);
        let rctx = loaders.resolution_ctx_at(content, offset as u32);
        let reached = reached_type(&site, &rctx)?;
        key_path::segment_owners(&reached, name, site.method.rules(), &rctx)
            .into_iter()
            .find_map(|(class, kind)| {
                self.class_member_declaration_location(
                    &class.fqn(),
                    name.strip_suffix("()").unwrap_or(name),
                    kind == SegmentKind::Property,
                )
            })
    }
}

/// The type the keys before the site's segment reach in its value.
fn reached_type(
    site: &PathSite<'_>,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolved = resolve_target_classes(site.value, AccessKind::Arrow, rctx);
    if resolved.is_empty() {
        return None;
    }
    let value = ResolvedType::types_joined(&resolved);
    let reached = key_path::path_type(
        &value,
        site.keys.iter().map(String::as_str),
        site.method.rules(),
        rctx,
    )?;
    Some(reached.ty)
}

/// The segment of an `ArrayHelper` key path the string around `offset`
/// holds, when it holds one.
fn path_site<'c>(
    content: &'c str,
    offset: usize,
    ctx: &FileContext,
    code: &CodeContext<'_>,
) -> Option<PathSite<'c>> {
    let (quote, quote_char) = code.open_string?;
    let call = code.enclosing_paren()?;
    let operator = call.callee_operator.filter(|operator| operator.is_static)?;
    let (method_name, _) = extract_identifier_backwards(content.get(..call.code_before)?)?;
    let class_text = content.get(..operator.code_before)?;
    let class_start = class_text
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
        .map_or(0, |i| i + 1);
    let written = &class_text[class_start..];
    if written.is_empty() {
        return None;
    }
    let class = ctx.resolve_name_at(written, class_start as u32);
    let method = path_method(&class, method_name)?;

    let argument = string_argument(content, offset, code, call)?;
    let in_key = match &argument.name {
        Some(name) => name == method.key_param,
        None => argument.index == 1,
    };
    if !in_key {
        return None;
    }
    let arguments_end = closing_call_paren(content, call.offset).unwrap_or(offset);
    let arguments =
        crate::text_scan::split_top_level_commas(content.get(call.offset + 1..arguments_end)?);
    let value = crate::yii::array_helper::text_argument(&arguments, 0, "array")?;
    let delimiter = delimiter(method, &arguments)?;

    let mut keys = match argument.position {
        StringArgumentPosition::Direct => Vec::new(),
        StringArgumentPosition::ArrayElement => {
            // The elements of the list before this one.
            let list = code
                .open_brackets
                .last()
                .filter(|bracket| bracket.byte == b'[' && bracket.offset > call.offset)?;
            let before = content.get(list.offset..quote)?.trim_end();
            let before = before.strip_suffix(',').unwrap_or(before);
            if before.len() > 1 {
                path_keys(method, &format!("{before}]"), delimiter)?
            } else {
                Vec::new()
            }
        }
        _ => return None,
    };

    // The keys this string holds before the segment.  A string with an
    // escape or an interpolation is read by none of the features here.
    let text = content.get(quote + 1..argument.string_end.max(offset))?;
    if text.contains('\\') || (quote_char == '"' && text.contains(['$', '{'])) {
        return None;
    }
    let in_string = offset - (quote + 1);
    let (start, end) = if method.split {
        let start = text[..in_string]
            .rfind(delimiter)
            .map_or(0, |i| i + delimiter.len_utf8());
        let end = text[in_string..]
            .find(delimiter)
            .map_or(text.len(), |i| in_string + i);
        if start > 0 {
            keys.extend(split_path(&text[..start - delimiter.len_utf8()], delimiter));
        }
        (start, end)
    } else {
        (0, text.len())
    };
    Some(PathSite {
        method,
        value,
        keys,
        start: quote + 1 + start,
        end: quote + 1 + end,
    })
}
