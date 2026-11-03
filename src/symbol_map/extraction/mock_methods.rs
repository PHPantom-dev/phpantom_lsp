//! Symbol spans for the method names a mock expectation spells as a
//! string.
//!
//! `$repo->method('findOpen')`, `$mailer->shouldReceive('send')` and
//! `Cache::shouldReceive('get')` each name a method of the mocked class
//! (or the facade's target) in a string literal.  Each name becomes a
//! [`DocblockMemberRef::MockedMethod`] member access on the expectation's
//! receiver, so navigation, references, rename, hover and the
//! unknown-member check treat it as the method it names.
//!
//! The extractor cannot see types, so it emits a span for every call of
//! one of these names with a literal argument: `$gate->allows('update')`
//! is spelled exactly like a Mockery expectation.  The consumers resolve
//! the receiver and keep only a mock (or, for `::`, a facade); see
//! [`crate::mock_methods`].

use mago_span::HasSpan;

use super::*;
use crate::mock_methods::{NameArgs, expectation_name_args};

/// Emit a span for each method name the instance call
/// `object->method(arguments)` names, if `method` is an expectation method.
pub(super) fn emit_mock_method_spans(
    object: &Expression<'_>,
    method: &str,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(kind) = expectation_name_args(method, false) else {
        return;
    };
    // `$mock->expects($this->once())->method('name')` configures the mock
    // the `expects()` was called on, and the invocation mocker in between
    // has forgotten which class that is.
    let receiver = match object {
        Expression::Call(Call::Method(call))
            if method.eq_ignore_ascii_case("method")
                && matches!(&call.method, ClassLikeMemberSelector::Identifier(ident)
                    if ident.value.eq_ignore_ascii_case(b"expects")) =>
        {
            call.object
        }
        _ => object,
    };
    emit_names(receiver, false, kind, arguments, ctx);
}

/// Emit a span for each method name the static call
/// `class::method(arguments)` names, if `method` is one of the facade's
/// expectation methods.
pub(super) fn emit_facade_mock_method_spans(
    class: &Expression<'_>,
    method: &str,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(kind) = expectation_name_args(method, true) else {
        return;
    };
    emit_names(class, true, kind, arguments, ctx);
}

/// Emit one span per method name `arguments` spell, as an access on
/// `receiver`.
fn emit_names(
    receiver: &Expression<'_>,
    is_static: bool,
    kind: NameArgs,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let mut names: Vec<(u32, u32)> = Vec::new();
    for (index, argument) in arguments.arguments.iter().enumerate() {
        if !kind.names_at(index) {
            break;
        }
        let Argument::Positional(positional) = argument else {
            break;
        };
        if positional.ellipsis.is_some() {
            break;
        }
        let value = positional.value;
        if index == 0
            && kind.takes_map()
            && let Some(elements) = crate::parser::array_literal_elements(value)
        {
            names.extend(
                elements
                    .iter()
                    .filter_map(crate::parser::array_element_key)
                    .filter_map(|key| method_name_literal(key, ctx.content)),
            );
            break;
        }
        if let Some(range) = method_name_literal(value, ctx.content) {
            names.push(range);
        }
    }
    if names.is_empty() {
        return;
    }

    let subject_text = expr_to_subject_text(receiver);
    if subject_text.is_empty() {
        return;
    }
    let receiver_span = receiver.span();
    for (start, end) in names {
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
                is_static,
                is_method_call: true,
                docblock_ref: DocblockMemberRef::MockedMethod,
                is_array_callable: false,
                is_nullsafe: false,
            },
        });
    }
}

/// The range inside the quotes of a string literal that holds a method
/// name.
///
/// A Mockery demeter chain (`'getConnection->query'`) is not one, and is
/// left alone.
fn method_name_literal(expr: &Expression<'_>, content: &str) -> Option<(u32, u32)> {
    let Expression::Literal(literal::Literal::String(literal)) = expr else {
        return None;
    };
    let start = literal.span.start.offset + 1;
    let end = literal.span.end.offset.checked_sub(1)?;
    let name = content.get(start as usize..end as usize)?;
    is_identifier(name).then_some((start, end))
}
