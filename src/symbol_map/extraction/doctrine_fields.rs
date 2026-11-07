//! Symbol spans for the entity fields a Doctrine repository call keys its
//! criteria and order by.
//!
//! `$users->findBy(['email' => $email], ['name' => 'ASC'])` names the
//! `email` and `name` fields of the repository's entity.  Each key becomes a
//! [`DocblockMemberRef::EntityField`] access on the call's receiver, so
//! navigation, references, rename, and hover treat it as the property it
//! maps.
//!
//! The extractor cannot see types, and `findBy()`, `findOneBy()`, and
//! `count()` are common names, so it emits a span for every such call
//! whose argument is an array literal.  The consumers resolve the receiver
//! and keep only a repository whose entity is known; see
//! [`crate::doctrine::managed_entity_classes`].

use super::*;

/// Emit a span for each field the instance call `object->method(arguments)`
/// keys an argument by, if `method` takes field names.
pub(super) fn emit_entity_field_spans(
    object: &Expression<'_>,
    method: &str,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !crate::doctrine::takes_field_names(method, 0) {
        return;
    }
    let mut keys: Vec<(u32, u32)> = Vec::new();
    for (index, argument) in arguments.arguments.iter().enumerate() {
        let value = match argument {
            Argument::Positional(positional) if positional.ellipsis.is_none() => {
                if !crate::doctrine::takes_field_names(method, index) {
                    continue;
                }
                positional.value
            }
            Argument::Named(named) => {
                if !crate::doctrine::field_keyed_parameter(
                    method,
                    crate::atom::bytes_to_str(named.name.value),
                ) {
                    continue;
                }
                named.value
            }
            _ => break,
        };
        let Some(elements) = crate::parser::array_literal_elements(value) else {
            continue;
        };
        keys.extend(
            elements
                .iter()
                .filter_map(crate::parser::array_element_key)
                .filter_map(|key| field_name_literal(key, ctx.content)),
        );
    }
    if keys.is_empty() {
        return;
    }

    let subject_text = expr_to_subject_text(object);
    if subject_text.is_empty() {
        return;
    }
    let receiver_span = object.span();
    for (start, end) in keys {
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::MemberAccess {
                subject_text: SubjectText::new(
                    subject_text.clone(),
                    receiver_span.start.offset,
                    receiver_span.end.offset,
                    ctx.content,
                ),
                member_name: crate::atom::atom(&ctx.content[start as usize..end as usize]),
                is_static: false,
                is_method_call: false,
                docblock_ref: DocblockMemberRef::EntityField,
                is_array_callable: false,
                is_nullsafe: false,
            },
        });
    }
}

/// The range inside the quotes of a string literal that holds a field
/// name.  An embedded object's field (`'address.city'`) is not one.
fn field_name_literal(expr: &Expression<'_>, content: &str) -> Option<(u32, u32)> {
    let Expression::Literal(literal::Literal::String(literal)) = expr else {
        return None;
    };
    let start = literal.span.start.offset + 1;
    let end = literal.span.end.offset.checked_sub(1)?;
    let name = content.get(start as usize..end as usize)?;
    is_identifier(name).then_some((start, end))
}
