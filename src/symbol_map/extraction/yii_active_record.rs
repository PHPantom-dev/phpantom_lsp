//! The attributes and relations a Yii Active Record call names (see
//! [`crate::yii::active_record`]).
//!
//! `$post->get('title')` names the `title` attribute of the record, which
//! becomes a [`DocblockMemberRef::YiiAttribute`] access on the receiver.
//! `$post->relation('author')` and `Post::query()->with('author.profile')`
//! name relations, which are kept as [`YiiRelationSite`]s, one per segment
//! of a path.
//!
//! The extractor cannot see types, and `get()` and `with()` are common
//! names, so it reads every such call in a Yii project; the consumers
//! resolve the receiver and keep only a record or a query for one.

use crate::atom::{Atom, atom};
use crate::symbol_map::YiiRelationSite;
use crate::yii::active_record::{NamedArgument, named_argument};

use super::*;

/// Read the names the instance call `object->method(arguments)` passes,
/// if `method` is one of a record's or a query's that takes them.
pub(super) fn emit_yii_active_record_spans(
    object: &Expression<'_>,
    method: &str,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.yii || named_argument(method, 0).is_none() {
        return;
    }
    for (index, argument) in arguments.arguments.iter().enumerate() {
        let Argument::Positional(argument) = argument else {
            break;
        };
        if argument.ellipsis.is_some() {
            break;
        }
        let Some(kind) = named_argument(method, index) else {
            continue;
        };
        match kind {
            NamedArgument::Attribute => {
                if let Some((start, end)) = name_literal(argument.value, ctx.content) {
                    emit_attribute(object, start, end, ctx);
                }
            }
            NamedArgument::RecordRelation => {
                if let Some((start, end)) = name_literal(argument.value, ctx.content) {
                    push_relation(object, false, start, end, atom(""), ctx);
                }
            }
            NamedArgument::QueryRelations => {
                if let Some(elements) = crate::parser::array_literal_elements(argument.value) {
                    for element in elements.iter() {
                        let path = match element {
                            ArrayElement::KeyValue(entry) => entry.key,
                            ArrayElement::Value(entry) => entry.value,
                            _ => continue,
                        };
                        emit_path(object, path, ctx);
                    }
                } else {
                    emit_path(object, argument.value, ctx);
                }
            }
        }
    }
}

/// The range inside the quotes of a string literal written as it reads,
/// with no escape or interpolation.
fn literal_range(expr: &Expression<'_>, content: &str) -> Option<(u32, u32)> {
    let (value, start, end) =
        crate::virtual_members::laravel::helpers::resolved_string_literal(expr, content)?;
    (content.get(start..end)? == value).then_some((start as u32, end as u32))
}

/// The range of a string literal that holds a single name.
fn name_literal(expr: &Expression<'_>, content: &str) -> Option<(u32, u32)> {
    let (start, end) = literal_range(expr, content)?;
    is_identifier(&content[start as usize..end as usize]).then_some((start, end))
}

/// A relation site for each segment of the path a string literal holds,
/// up to the first that is not a name.
fn emit_path(object: &Expression<'_>, expr: &Expression<'_>, ctx: &mut ExtractionCtx<'_>) {
    let Some((start, end)) = literal_range(expr, ctx.content) else {
        return;
    };
    let text = &ctx.content[start as usize..end as usize];
    let mut offset = start;
    for segment in text.split('.') {
        if !is_identifier(segment) {
            return;
        }
        let path = atom(text[..(offset - start) as usize].trim_end_matches('.'));
        let segment_end = offset + segment.len() as u32;
        push_relation(object, true, offset, segment_end, path, ctx);
        offset = segment_end + 1;
    }
}

fn push_relation(
    object: &Expression<'_>,
    on_query: bool,
    start: u32,
    end: u32,
    path: Atom,
    ctx: &mut ExtractionCtx<'_>,
) {
    let subject = ctx.spine_texts.subject_text_of(object, ctx.content);
    ctx.yii_relations.push(YiiRelationSite {
        start,
        end,
        subject,
        on_query,
        path,
    });
}

fn emit_attribute(object: &Expression<'_>, start: u32, end: u32, ctx: &mut ExtractionCtx<'_>) {
    let subject_text = ctx.spine_texts.subject_text_of(object, ctx.content);
    ctx.spans.push(SymbolSpan {
        start,
        end,
        kind: SymbolKind::MemberAccess {
            subject_text,
            member_name: atom(&ctx.content[start as usize..end as usize]),
            is_static: false,
            is_method_call: false,
            docblock_ref: DocblockMemberRef::YiiAttribute,
            is_array_callable: false,
            is_nullsafe: false,
        },
    });
}
