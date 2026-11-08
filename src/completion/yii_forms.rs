//! Completion of the properties Yii form fields and hydration maps name.
//!
//! `Field::text($form, '|')`, a key of the map a form hydrator is given
//! (`populate($form, $data, ['|' …`), and a key of the map in the
//! `ArrayData` a hydrator fills an object from
//! (`hydrate($post, new ArrayData($data, ['|' …`) offer the properties of
//! the object, as extraction reads the same calls (see
//! `symbol_map/extraction/yii_forms.rs` and [`crate::yii::forms`]).

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::extract_identifier_backwards;
use crate::completion::source::code_context::{CodeContext, OpenBracket};
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::type_engine::resolver::resolve_target_classes_expr;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, FileContext, ResolvedType};
use crate::yii::forms::{
    ARRAY_DATA, OBJECT_MAP, is_field_method, is_hydrate_method, is_populate_method,
    is_property_name,
};

/// Where the string being completed names a property, and of what.
struct PropertySite<'c> {
    /// The object whose property it names, as written.
    object: &'c str,
    /// Whether the object has to be a form model for the string to name
    /// one of its properties.
    form_model: bool,
    /// Where the property starts in the string, after a tabular index.
    start: usize,
    /// The keys the map already has.
    used: Vec<String>,
}

impl Backend {
    /// Complete a property a form field or a hydration map names.  `None`
    /// when the cursor is on neither.
    pub(crate) fn try_yii_form_property_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let cursor = position_to_offset(content, position) as usize;
        let site = property_site(content, cursor, quote, ctx, code)?;
        let prefix = content.get(site.start..cursor)?;
        if !prefix.is_empty() && !is_property_name(prefix) {
            return None;
        }

        let subject = SubjectExpr::parse(site.object);
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let mut classes = ResolvedType::into_arced_classes(resolve_target_classes_expr(
            &subject,
            AccessKind::Arrow,
            &loaders.resolution_ctx_at(content, cursor as u32),
        ));
        if site.form_model {
            classes = crate::yii::forms::form_model_classes(classes, class_loader);
        }
        let class = classes.first()?;
        let class = crate::virtual_members::resolve_class_fully_maybe_cached(
            class,
            class_loader,
            Some(&self.resolved_class_cache),
        );

        let edit_range = Range {
            start: offset_to_position(content, site.start),
            end: position,
        };
        let mut seen: Vec<&str> = Vec::new();
        let items: Vec<CompletionItem> = class
            .properties
            .iter()
            .filter(|property| !property.is_static)
            .filter_map(|property| {
                let name = property.name.trim_start_matches('$');
                let fresh = !seen.contains(&name)
                    && name
                        .as_bytes()
                        .get(..prefix.len())
                        .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
                    && !site.used.iter().any(|used| used == name);
                seen.push(name);
                fresh.then_some((name, property))
            })
            .enumerate()
            .map(|(at, (name, property))| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::Property),
                detail: property.type_hint.as_ref().map(|ty| ty.to_string()),
                sort_text: Some(format!("{at:05}")),
                filter_text: Some(name.to_string()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: name.to_string(),
                })),
                ..Default::default()
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }
}

/// What the string opened at `quote` names a property of, if anything.
fn property_site<'c>(
    content: &'c str,
    cursor: usize,
    quote: usize,
    ctx: &FileContext,
    code: &CodeContext<'_>,
) -> Option<PropertySite<'c>> {
    let calls: Vec<&OpenBracket> = code
        .open_brackets
        .iter()
        .filter(|bracket| bracket.byte == b'(')
        .collect();
    let (&call, outer) = calls.split_last()?;
    let argument = string_argument(content, cursor, code, call)?;
    let (method, _) = extract_identifier_backwards(&content[..call.code_before])?;
    let is_map_key = matches!(
        argument.position,
        StringArgumentPosition::ArrayKey | StringArgumentPosition::ArrayElement
    );
    let takes = |index: usize, name: &str| match &argument.name {
        Some(label) => label == name,
        None => argument.index == index,
    };

    if call.callee_operator.is_some() {
        if is_field_method(method)
            && argument.position == StringArgumentPosition::Direct
            && takes(1, "property")
        {
            // A tabular index (`[0]content`) comes before the property.
            let text = content.get(quote + 1..cursor)?;
            let start = match text.rfind(']') {
                Some(end) if text.starts_with('[') => end + 1,
                _ => 0,
            };
            return Some(PropertySite {
                object: argument_text(content, call, quote, 0, "formModel")?,
                form_model: true,
                start: quote + 1 + start,
                used: Vec::new(),
            });
        }
        if is_populate_method(method) && is_map_key && takes(2, "map") {
            return Some(PropertySite {
                object: argument_text(content, call, quote, 0, "model")?,
                form_model: true,
                start: quote + 1,
                used: argument.used_keys,
            });
        }
        return None;
    }

    // `new ArrayData($data, ['|' …`, or `new ObjectMap(['|' …` given to
    // one in its place, given to `hydrate()`.
    if !is_map_key {
        return None;
    }
    let (mut class, mut new_at) = instantiated(content, call, ctx)?;
    let mut outer = outer;
    if class.eq_ignore_ascii_case(OBJECT_MAP) {
        if !takes(0, "map") {
            return None;
        }
        let (&data, rest) = outer.split_last()?;
        if data.callee_operator.is_some() || !is_argument(content, data, new_at, 1, "map") {
            return None;
        }
        (class, new_at) = instantiated(content, data, ctx)?;
        outer = rest;
    } else if !takes(1, "map") {
        return None;
    }
    if !class.eq_ignore_ascii_case(ARRAY_DATA) {
        return None;
    }
    let &hydrate = outer.last()?;
    let (method, _) = extract_identifier_backwards(&content[..hydrate.code_before])?;
    if hydrate.callee_operator.is_none()
        || !is_hydrate_method(method)
        || !is_argument(content, hydrate, new_at, 1, "data")
    {
        return None;
    }
    Some(PropertySite {
        object: argument_text(content, hydrate, new_at, 0, "object")?,
        form_model: false,
        start: quote + 1,
        used: argument.used_keys,
    })
}

/// The class a `new` written before `call` instantiates, fully qualified,
/// with the offset of the `new`.
fn instantiated(content: &str, call: &OpenBracket, ctx: &FileContext) -> Option<(String, usize)> {
    let text = content.get(..call.code_before)?.trim_end();
    let start = text
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
        .map_or(0, |i| i + 1);
    let written = &text[start..];
    let keyword = text[..start].trim_end();
    let new_at = keyword.len().checked_sub(3)?;
    if written.is_empty()
        || !keyword[new_at..].eq_ignore_ascii_case("new")
        || keyword[..new_at]
            .bytes()
            .next_back()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let resolved = ctx.resolve_name_at(written, start as u32);
    Some((resolved.trim_start_matches('\\').to_string(), new_at))
}

/// The text of the argument of `call` at `index`, passed in place or by
/// `name`, among the arguments written before `end`.
fn argument_text<'c>(
    content: &'c str,
    call: &OpenBracket,
    end: usize,
    index: usize,
    name: &str,
) -> Option<&'c str> {
    let arguments = crate::text_scan::split_top_level_commas(content.get(call.offset + 1..end)?);
    let mut found = None;
    for (at, argument) in arguments.iter().enumerate() {
        match crate::call_args::split_named_text_arg(argument) {
            (Some(label), value) if label == name => return Some(value),
            (None, value) if at == index => found = Some(value),
            _ => {}
        }
    }
    found.filter(|value| !value.is_empty())
}

/// Whether the expression starting at `at` is the argument of `call` at
/// `index`, passed in place or by `name`.
fn is_argument(content: &str, call: &OpenBracket, at: usize, index: usize, name: &str) -> bool {
    let Some(text) = content.get(call.offset + 1..at) else {
        return false;
    };
    let commas = crate::text_scan::top_level_commas(text).collect::<Vec<_>>();
    // What is written between the last comma and the argument is its
    // label, if it has one.
    let label = text[commas.last().map_or(0, |&i| i + 1)..].trim();
    match label.strip_suffix(':') {
        Some(label) => label.trim_end() == name,
        None => label.is_empty() && commas.len() == index,
    }
}
