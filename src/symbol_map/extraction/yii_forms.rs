//! The properties Yii form fields and hydration maps name (see
//! [`crate::yii::forms`]).
//!
//! `Field::text($form, 'title')` and `$fieldFactory->text($form, 'title')`
//! name the `title` property of `$form`, and so do the keys of the map a
//! `FormHydrator` is given (`populate($form, $data, ['title' => 'post'])`)
//! and of the map in the `ArrayData` a hydrator fills an object from
//! (`hydrate($post, new ArrayData($data, ['title' => 'post']))`).  Each is
//! a [`DocblockMemberRef::YiiProperty`] access on the object.
//!
//! The field helpers and the form hydrator's methods have names any class
//! could have, so their accesses are kept only where the object resolves
//! to a form model.  An `ArrayData` is a class the file names, so its map
//! is read on sight.

use mago_span::HasSpan;

use super::*;
use crate::virtual_members::laravel::helpers::extract_string_literal;
use crate::yii::forms::{
    ARRAY_DATA, OBJECT_MAP, field_property, is_field_method, is_hydrate_method, is_populate_method,
    is_property_name,
};

/// Emit the properties the method call `method(arguments)`, written with
/// `->` or `::`, names on the object it is given.
pub(in crate::symbol_map::extraction) fn emit_yii_form_property_spans(
    method: &str,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.yii {
        return;
    }
    if is_field_method(method) {
        let (Some(model), Some(property)) = (
            argument(arguments, 0, "formModel"),
            argument(arguments, 1, "property"),
        ) else {
            return;
        };
        let Some((text, start, _)) = extract_string_literal(property, ctx.content) else {
            return;
        };
        if text.contains(['\\', '$', '{']) {
            return;
        }
        let Some((offset, name)) = field_property(text) else {
            return;
        };
        let start = (start + offset) as u32;
        push_property(model, start, start + name.len() as u32, true, ctx);
    } else if is_populate_method(method) {
        let (Some(model), Some(map)) = (
            argument(arguments, 0, "model"),
            argument(arguments, 2, "map"),
        ) else {
            return;
        };
        emit_map_keys(model, map, true, ctx);
    } else if is_hydrate_method(method) {
        let (Some(object), Some(data)) = (
            argument(arguments, 0, "object"),
            argument(arguments, 1, "data"),
        ) else {
            return;
        };
        let Some(arguments) = instantiation_of(data, ARRAY_DATA, ctx) else {
            return;
        };
        let Some(map) = argument(arguments, 1, "map") else {
            return;
        };
        let map = instantiation_of(map, OBJECT_MAP, ctx)
            .and_then(|arguments| argument(arguments, 0, "map"))
            .unwrap_or(map);
        emit_map_keys(object, map, false, ctx);
    }
}

/// The argument at `index`, passed in place or by `name`.
fn argument<'a>(
    arguments: &ArgumentList<'a>,
    index: usize,
    name: &str,
) -> Option<&'a Expression<'a>> {
    for (at, argument) in arguments.arguments.iter().enumerate() {
        match argument {
            Argument::Positional(positional) if positional.ellipsis.is_none() => {
                if at == index {
                    return Some(positional.value);
                }
            }
            Argument::Named(named) => {
                if bytes_to_str(named.name.value) == name {
                    return Some(named.value);
                }
            }
            Argument::Positional(_) => return None,
        }
    }
    None
}

/// The arguments of `expr` when it is a `new` of `class`.
fn instantiation_of<'a>(
    expr: &'a Expression<'a>,
    class: &str,
    ctx: &ExtractionCtx<'_>,
) -> Option<&'a ArgumentList<'a>> {
    let Expression::Instantiation(instantiation) = expr else {
        return None;
    };
    let Expression::Identifier(identifier) = instantiation.class else {
        return None;
    };
    let written = bytes_to_str(identifier.value());
    ctx.resolved_name_at(identifier.span().start.offset)
        .unwrap_or(written)
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(class)
        .then_some(instantiation.argument_list.as_ref()?)
}

/// Emit the properties of `object` the keys of the array `map` name.
///
/// A key's value can be a map of its own for the object in that property,
/// whose keys belong to that object's class, which is not read here.
fn emit_map_keys(
    object: &Expression<'_>,
    map: &Expression<'_>,
    form_model: bool,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(elements) = crate::parser::array_literal_elements(map) else {
        return;
    };
    for key in elements.iter().filter_map(crate::parser::array_element_key) {
        let Some((name, start, end)) = extract_string_literal(key, ctx.content) else {
            continue;
        };
        if is_property_name(name) {
            push_property(object, start as u32, end as u32, form_model, ctx);
        }
    }
}

/// Push the access to the property of `object` spelled at `start..end`.
fn push_property(
    object: &Expression<'_>,
    start: u32,
    end: u32,
    form_model: bool,
    ctx: &mut ExtractionCtx<'_>,
) {
    let subject_text = expr_to_subject_text(object);
    if subject_text.is_empty() {
        return;
    }
    let span = object.span();
    ctx.spans.push(SymbolSpan {
        start,
        end,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::new(
                subject_text,
                span.start.offset,
                span.end.offset,
                ctx.content,
            ),
            member_name: crate::atom::atom(&ctx.content[start as usize..end as usize]),
            is_static: false,
            is_method_call: false,
            docblock_ref: DocblockMemberRef::YiiProperty { form_model },
            is_array_callable: false,
            is_nullsafe: false,
        },
    });
}
