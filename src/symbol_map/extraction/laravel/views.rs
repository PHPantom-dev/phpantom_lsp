//! View names: the view factory and the helpers and facades that reach it,
//! a mailable's templates, and the Blade sections and stacks the
//! preprocessor lowers to marker calls.

use super::*;
use crate::virtual_members::laravel::helpers::string_literal_at_range;

/// Emit the section- or stack-name span for one of the marker calls the
/// Blade preprocessor lowers `@yield`, `@section`, `@stack`, `@push` and
/// their helpers to, when `name` is one of them.
///
/// `@pushIf` is the odd one out: Laravel folds everything before the last
/// comma into the condition, so the stack it pushes to is the last
/// argument rather than the first, and it lowers to a marker of its own to
/// say so.
pub(in crate::symbol_map::extraction) fn try_emit_blade_block_span(
    name: &str,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    use crate::blade::markers::{
        PUSH_IF as PUSH_IF_MARKER, SECTION as SECTION_MARKER, STACK as STACK_MARKER,
    };
    use crate::symbol_map::LaravelStringKind;

    let (kind, index) = if name.eq_ignore_ascii_case(SECTION_MARKER) {
        (LaravelStringKind::Section, 0)
    } else if name.eq_ignore_ascii_case(STACK_MARKER) {
        (LaravelStringKind::Stack, 0)
    } else if name.eq_ignore_ascii_case(PUSH_IF_MARKER) {
        (
            LaravelStringKind::Stack,
            argument_list.arguments.len().saturating_sub(1),
        )
    } else {
        return;
    };
    emit_laravel_string_span(kind, false, index, argument_list, content, spans);
}

/// The [`try_emit_laravel_string_span_at`] variant for a view argument that
/// may name several candidates at once.
///
/// `@includeFirst(['custom.header', 'partials.header'])` and its
/// `@componentFirst` / `@extendsFirst` siblings, and the view factory's
/// `first()`, put their names in an array literal and render whichever
/// exists, so every entry is a template the call can reach.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_view_span_at(
    index: usize,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let kind = crate::symbol_map::LaravelStringKind::View;
    let Some(arg) = argument_list.arguments.iter().nth(index) else {
        return;
    };
    let Some(elements) = crate::parser::array_literal_elements(arg.value()) else {
        return push_laravel_string_span(kind, false, false, arg.value(), content, spans);
    };
    // Only one candidate of the list is rendered, and which one depends on
    // what exists on disk, so a candidate that names nothing is the shape
    // working as intended rather than a typo.
    for element in elements.iter() {
        if let ArrayElement::Value(value) = element {
            push_laravel_string_span(kind, false, true, value.value, content, spans);
        }
    }
}

/// The `Illuminate\Mail\Mailables\Content` constructor arguments that name a
/// template, in the order the constructor declares them.
///
/// `with:` holds the data and follows them; every other argument of a
/// `Content` is a setting rather than a view.
const MAILABLE_CONTENT_VIEW_ARGS: [&str; 4] = ["view", "html", "text", "markdown"];

/// Push a view span for every template a
/// `new Content(view: 'emails.orders.shipped', with: […])` names.
///
/// A mailable's `content()` returns one of these, and any of `view:`,
/// `html:`, `text:`, and `markdown:` names a template it renders.  The
/// arguments are usually named, but the constructor takes them in
/// [`MAILABLE_CONTENT_VIEW_ARGS`] order, so a positional call names the same
/// templates.
pub(in crate::symbol_map::extraction) fn try_emit_mailable_content_view_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    for (index, argument) in argument_list.arguments.iter().enumerate() {
        let names_view = match argument {
            Argument::Named(named) => {
                let name = bytes_to_str(named.name.value);
                MAILABLE_CONTENT_VIEW_ARGS
                    .iter()
                    .any(|arg| name.eq_ignore_ascii_case(arg))
            }
            Argument::Positional(_) => index < MAILABLE_CONTENT_VIEW_ARGS.len(),
        };
        if names_view {
            push_laravel_string_span(
                crate::symbol_map::LaravelStringKind::View,
                false,
                false,
                argument.value(),
                content,
                spans,
            );
        }
    }
}

/// Whether `member_name` is a `Illuminate\Mail\Mailable` method whose first
/// argument names a template.
///
/// `html()` is not among them: it takes the rendered markup itself rather
/// than the name of a view, unlike the `Content` argument of the same name.
pub(in crate::symbol_map::extraction) fn is_mailable_view_method(member_name: &str) -> bool {
    matches!(
        member_name.to_ascii_lowercase().as_str(),
        "view" | "text" | "markdown"
    )
}

/// The argument index at which a view-factory method names its template,
/// or `None` when the method names none.
///
/// `first()` names a list of candidates rather than one template, which
/// [`try_emit_laravel_view_span_at`] reads either way.  `renderEach()` names
/// two, so it is read by [`try_emit_render_each_view_spans`] instead.
pub(in crate::symbol_map::extraction) fn view_factory_method_name_index(
    member_name: &str,
) -> Option<usize> {
    match member_name.to_ascii_lowercase().as_str() {
        "make" | "first" | "exists" => Some(0),
        "renderwhen" | "renderunless" => Some(1),
        _ => None,
    }
}

/// The argument indices at which `Factory::renderEach($view, $data,
/// $iterator, $empty)` names a template: the partial rendered per entry,
/// and the one rendered when the collection is empty.
const RENDER_EACH_VIEW_ARGS: [usize; 2] = [0, RENDER_EACH_EMPTY_ARG];

/// The argument at which `renderEach()` takes the template it falls back
/// to, which is the one that doubles as raw markup.
const RENDER_EACH_EMPTY_ARG: usize = 3;

/// Whether the view factory's `member_name` names a template in its
/// positional argument `index`.
pub(crate) fn view_factory_names_view_at(member_name: &str, index: usize) -> bool {
    view_factory_method_name_index(member_name) == Some(index)
        || (is_render_each_method(member_name) && RENDER_EACH_VIEW_ARGS.contains(&index))
}

/// Whether `member_name` is the view factory's `renderEach()`, the PHP
/// spelling of Blade's `@each`.
pub(crate) fn is_render_each_method(member_name: &str) -> bool {
    member_name.eq_ignore_ascii_case("renderEach")
}

/// Push a view span for each template a `renderEach()` call names.
///
/// The `$empty` argument doubles as raw markup: Laravel renders it as a
/// template unless it carries the `raw|` prefix, which is also the
/// default, so a string spelled that way names nothing.
pub(in crate::symbol_map::extraction) fn try_emit_render_each_view_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    for index in RENDER_EACH_VIEW_ARGS {
        if names_a_render_each_template(index, argument_list, content) {
            try_emit_laravel_string_span_at(
                crate::symbol_map::LaravelStringKind::View,
                index,
                argument_list,
                content,
                spans,
            );
        }
    }
}

/// Whether the `renderEach()` argument at `index` names a template.
///
/// Only the fallback can fail to: Laravel renders it as a template unless
/// it carries the `raw|` prefix, which is also its default, so a string
/// spelled that way is markup rather than a name.
fn names_a_render_each_template(
    index: usize,
    argument_list: &ArgumentList<'_>,
    content: &str,
) -> bool {
    if index != RENDER_EACH_EMPTY_ARG {
        return true;
    }
    let Some(arg) = argument_list.arguments.iter().nth(index) else {
        return false;
    };
    match string_literal_at_range(arg.value(), content) {
        Some((text, _, _)) => !text.starts_with("raw|"),
        None => true,
    }
}

/// The class a receiver has to be for the method call it receives to name a
/// template, when nothing about the receiver's *spelling* settles it.
///
/// Returns the expected receiver alongside the argument indices that name a
/// template.  Kept allocation-free: this runs on every method call in every
/// file, and `make`, `first`, and `view` are common enough method names that
/// lowercasing each one would be a real cost.
pub(in crate::symbol_map::extraction) fn view_receiver_method(
    member_name: &str,
) -> Option<(ViewReceiverClass, &'static [usize])> {
    const FIRST: &[usize] = &[0];
    const SECOND: &[usize] = &[1];
    // Every name below starts with one of these, so a method call that
    // cannot be a render is turned away on a single byte.
    if !matches!(
        member_name.as_bytes().first().map(u8::to_ascii_lowercase),
        Some(b'm' | b'f' | b'e' | b'v' | b't' | b'r')
    ) {
        return None;
    }
    if member_name.eq_ignore_ascii_case("make")
        || member_name.eq_ignore_ascii_case("first")
        || member_name.eq_ignore_ascii_case("exists")
    {
        return Some((ViewReceiverClass::Factory, FIRST));
    }
    if member_name.eq_ignore_ascii_case("view") || member_name.eq_ignore_ascii_case("markdown") {
        return Some((ViewReceiverClass::MailTemplate, FIRST));
    }
    if member_name.eq_ignore_ascii_case("text") {
        return Some((ViewReceiverClass::Mailable, FIRST));
    }
    if member_name.eq_ignore_ascii_case("renderWhen")
        || member_name.eq_ignore_ascii_case("renderUnless")
    {
        return Some((ViewReceiverClass::Factory, SECOND));
    }
    if is_render_each_method(member_name) {
        return Some((ViewReceiverClass::Factory, &RENDER_EACH_VIEW_ARGS));
    }
    None
}

/// Record every template a method call names, for the lazy pass that
/// settles what its receiver is (see [`ViewReceiverSite`]).
///
/// The names are read exactly as the confirmed spellings read them, so a
/// site that turns out to be a render contributes the same spans a
/// syntactically recognised one would.
pub(in crate::symbol_map::extraction) fn record_view_receiver_sites(
    receiver: ViewReceiverClass,
    name_indices: &[usize],
    argument_list: &ArgumentList<'_>,
    content: &str,
    sites: &mut Vec<ViewReceiverSite>,
) {
    let mut spans = Vec::new();
    for index in name_indices {
        if names_a_render_each_template(*index, argument_list, content) {
            try_emit_laravel_view_span_at(*index, argument_list, content, &mut spans);
        }
    }
    for span in spans {
        let SymbolKind::LaravelStringKey {
            key, is_optional, ..
        } = span.kind
        else {
            continue;
        };
        sites.push(ViewReceiverSite {
            start: span.start,
            end: span.end,
            key,
            is_optional,
            receiver,
        });
    }
}

/// Whether `object` is a bare `view()` (or `\view()`) helper call, which
/// returns the view factory itself rather than a rendered view.
pub(in crate::symbol_map::extraction) fn is_laravel_view_factory_call(
    object: &Expression<'_>,
) -> bool {
    is_bare_helper_call(object, "view")
}
