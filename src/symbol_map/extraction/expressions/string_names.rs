//! Symbols plain PHP names in a string literal: the method of an array
//! callable (`[Foo::class, 'bar']`) and the variables `compact()` lists.

use mago_syntax::cst::sequence::TokenSeparatedSequence;

use super::*;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// Detect an array-callable literal — `[Class::class, 'method']` or
/// `[$object, 'method']` — and emit a [`SymbolKind::MemberAccess`] span over
/// the method-name string so that go-to-definition, references, and rename
/// treat it like a real `Class::method` / `$object->method` reference.
///
/// This is the shape Laravel routes use for controller actions
/// (`Route::get('/', [IndexPageController::class, 'indexPage'])`), but the
/// pattern is generic PHP (event listeners, `array_map`, etc.).
///
/// Only fires for a two-element array whose first element is a `::class`
/// constant or a variable and whose second element is a plain string literal.
pub(super) fn try_emit_array_callable_span(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    // Exactly two positional (value) elements: `[<callee>, '<method>']`.
    let mut values = elements.iter().filter_map(|el| match el {
        ArrayElement::Value(v) => Some(v.value),
        _ => None,
    });
    let (Some(first), Some(second), None) = (values.next(), values.next(), values.next()) else {
        return;
    };
    // The whole array must have been exactly those two elements (no
    // key/value or variadic entries mixed in).
    if elements.iter().count() != 2 {
        return;
    }

    // Second element must be a plain string literal naming a method;
    // anything else (`[$a, 'some text']`) is data, not a callable.
    let Some((member_name, inner_start, inner_end)) = resolved_string_literal(second, content)
    else {
        return;
    };
    if !is_identifier(member_name) {
        return;
    }

    // First element determines the subject and access kind.
    let (subject_text, subject_span, is_static) = match first {
        // `Class::class` — static-style access on the class name.
        Expression::Access(Access::ClassConstant(cca)) => {
            if !crate::parser::is_class_fetch(cca) {
                return;
            }
            (expr_to_subject_text(cca.class), cca.class.span(), true)
        }
        // `$this` / `$object` — instance-style access.
        Expression::Variable(Variable::Direct(dv)) => {
            (bytes_to_str(dv.name).to_string(), dv.span, false)
        }
        _ => return,
    };
    if subject_text.is_empty() {
        return;
    }

    spans.push(SymbolSpan {
        start: inner_start as u32,
        end: inner_end as u32,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::new(
                subject_text,
                subject_span.start.offset,
                subject_span.end.offset,
                content,
            ),
            member_name: crate::atom::atom(member_name),
            is_static,
            is_method_call: true,
            docblock_ref: DocblockMemberRef::No,
            is_array_callable: true,
            is_nullsafe: false,
        },
    });
}

/// If `argument_list` belongs to a `compact()` call, emit one
/// [`SymbolKind::CompactVariable`] span per string-literal variable name,
/// including names inside (possibly nested) array-literal arguments.
pub(super) fn try_emit_compact_string_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    for arg in argument_list.arguments.iter() {
        emit_compact_name_spans(arg.value(), content, spans);
    }
}

/// Emit [`SymbolKind::CompactVariable`] spans for one `compact()` argument:
/// a string literal names a variable directly, and an array literal is
/// descended into recursively so `compact(['a', ['b']])` covers both names.
fn emit_compact_name_spans(expr: &Expression<'_>, content: &str, spans: &mut Vec<SymbolSpan>) {
    match expr {
        Expression::Literal(_) => {
            let Some((name, start, end)) = resolved_string_literal(expr, content) else {
                return;
            };
            spans.push(SymbolSpan {
                start: start as u32,
                end: end as u32,
                kind: SymbolKind::CompactVariable {
                    name: crate::atom::atom(name),
                },
            });
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                emit_compact_name_elem_spans(elem, content, spans);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                emit_compact_name_elem_spans(elem, content, spans);
            }
        }
        _ => {}
    }
}

/// Emit spans for one element of an array passed to `compact()`. Keys are
/// ignored; values are variable names or nested arrays.
fn emit_compact_name_elem_spans(
    elem: &ArrayElement<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match elem {
        ArrayElement::KeyValue(kv) => emit_compact_name_spans(kv.value, content, spans),
        ArrayElement::Value(v) => emit_compact_name_spans(v.value, content, spans),
        ArrayElement::Variadic(s) => emit_compact_name_spans(s.value, content, spans),
        ArrayElement::Missing(_) => {}
    }
}
