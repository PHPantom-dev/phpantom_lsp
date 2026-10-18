//! The span every Laravel string family is emitted through: one string
//! literal naming a key of some [`LaravelStringKind`], found at an argument
//! picked by position or by parameter name.
//!
//! [`LaravelStringKind`]: crate::symbol_map::LaravelStringKind

use super::*;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// If the first argument of `argument_list` is a non-empty, non-interpolated
/// string literal, push a [`SymbolKind::LaravelStringKey`] span covering the
/// string content (inside the quotes) onto `spans`.
///
/// Called by the `config()` function-call extractor and the
/// `Config::get()` / `Config::set()` static-call extractor so that
/// find-references and go-to-definition for Laravel config keys can use
/// the pre-built symbol map instead of re-parsing every file on demand.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_string_span(
    kind: crate::symbol_map::LaravelStringKind,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    if kind == crate::symbol_map::LaravelStringKind::Trans {
        if let Some(key) = argument_expr_for_parameter(argument_list, "key") {
            push_laravel_string_span(kind, false, false, key, content, spans);
        }
        return;
    }
    emit_laravel_string_span(kind, false, 0, argument_list, content, spans);
}

/// The [`try_emit_laravel_string_span`] variant for a helper that does not
/// lead with its key: `Route::view('/about', 'pages.about')` names the URI
/// first and the template second.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_string_span_at(
    kind: crate::symbol_map::LaravelStringKind,
    index: usize,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    emit_laravel_string_span(kind, false, index, argument_list, content, spans);
}

/// The [`try_emit_laravel_string_span`] variant for the variadic predicates
/// that take a list of names rather than one: `Route::is('admin.*',
/// 'account.*')` asks whether the current route is any of them, so every
/// argument names one.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_string_spans_all(
    kind: crate::symbol_map::LaravelStringKind,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    for argument in argument_list.arguments.iter() {
        push_laravel_string_span(kind, false, false, argument.value(), content, spans);
    }
}

/// Emit a config-backed resource name from a parameter selected by name or
/// positional slot.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_config_resource_span_for_parameter(
    resource: crate::symbol_map::LaravelConfigResource,
    shape: crate::symbol_map::laravel_resources::ResourceArgumentShape,
    access: crate::symbol_map::laravel_resources::ResourceAccess,
    argument_list: &ArgumentList<'_>,
    parameter: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(argument) = argument_expr_for_parameter(argument_list, parameter) else {
        return;
    };
    let elements = match argument {
        Expression::Array(array) if shape.accepts_array() => Some(&array.elements),
        Expression::LegacyArray(array) if shape.accepts_array() => Some(&array.elements),
        _ => None,
    };
    if let Some(elements) = elements {
        for element in elements.iter() {
            let value = match element {
                ArrayElement::KeyValue(element) => element.value,
                ArrayElement::Value(element) => element.value,
                ArrayElement::Variadic(_) | ArrayElement::Missing(_) => continue,
            };
            push_laravel_string_span(
                crate::symbol_map::LaravelStringKind::ConfigResource(resource),
                access.is_write(),
                access.is_optional(),
                value,
                content,
                spans,
            );
        }
    } else if shape.accepts_scalar() {
        push_laravel_string_span(
            crate::symbol_map::LaravelStringKind::ConfigResource(resource),
            access.is_write(),
            access.is_optional(),
            argument,
            content,
            spans,
        );
    }
}

pub(super) fn argument_expr_for_parameter<'a>(
    argument_list: &ArgumentList<'a>,
    parameter: &str,
) -> Option<&'a Expression<'a>> {
    for argument in argument_list.arguments.iter() {
        if let Argument::Named(named) = argument
            && bytes_to_str(named.name.value) == parameter
        {
            return Some(named.value);
        }
    }
    argument_list
        .arguments
        .iter()
        .find_map(|argument| match argument {
            Argument::Positional(positional) => Some(positional.value),
            Argument::Named(_) => None,
        })
}

fn partial_argument_expr_for_parameter<'a>(
    argument_list: &PartialArgumentList<'a>,
    parameter: &str,
) -> Option<&'a Expression<'a>> {
    for argument in argument_list.arguments.iter() {
        if let PartialArgument::Named(named) = argument
            && bytes_to_str(named.name.value) == parameter
        {
            return Some(named.value);
        }
    }
    argument_list
        .arguments
        .iter()
        .find_map(|argument| match argument {
            PartialArgument::Positional(positional) => Some(positional.value),
            _ => None,
        })
}

/// Apply `push` to each value of `expr`, which Laravel accepts as either a
/// single value or an array of them.
///
/// A key/value entry is skipped: every argument shaped this way is a list.
pub(super) fn for_each_value<'ast>(
    expr: &Expression<'ast>,
    mut push: impl FnMut(&Expression<'ast>),
) {
    let Some(elements) = crate::parser::array_literal_elements(expr) else {
        push(expr);
        return;
    };
    for element in elements.iter() {
        if let ArrayElement::Value(value) = element {
            push(value.value);
        }
    }
}

pub(super) fn emit_laravel_string_span(
    kind: crate::symbol_map::LaravelStringKind,
    is_write: bool,
    index: usize,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().nth(index) else {
        return;
    };
    push_laravel_string_span(kind, is_write, false, first_arg.value(), content, spans);
}

/// Emit the span for one string-literal expression that names a key.
///
/// The key compared against declared config/route/view/translation names is
/// the literal's unescaped runtime `value` (`'it\'s'` names `it's`), not the
/// raw source between the quotes; the span still covers the source text.
pub(super) fn push_laravel_string_span(
    kind: crate::symbol_map::LaravelStringKind,
    is_write: bool,
    is_optional: bool,
    expr: &Expression<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some((mut key, inner_start, inner_end)) = resolved_string_literal(expr, content) else {
        return;
    };
    let inner_start = inner_start as u32;
    let mut inner_end = inner_end as u32;

    if kind == crate::symbol_map::LaravelStringKind::Config && !is_write && !key.contains('.') {
        // A bare read names a config file, not one of its keys. A write can
        // establish that whole file's namespace with an opaque runtime value.
        return;
    }

    // A container argument spelled as a class name is a class reference rather
    // than a binding key: it resolves without the container's table.
    if kind == crate::symbol_map::LaravelStringKind::ContainerBinding && key.contains('\\') {
        return;
    }

    if kind == crate::symbol_map::LaravelStringKind::Command
        && let Some(name_len) = command_name_length(key)
    {
        // The string may carry inline arguments (`'app:sync --limit=50'`,
        // parsed by Symfony's `StringInput` at runtime); only the first
        // token is the command name.
        key = &key[..name_len];
        inner_end = inner_start + name_len as u32;
    }

    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelStringKey {
            key: normalised_key(kind, key),
            kind,
            is_write,
            is_optional,
        },
    });
}

/// Length of the leading command-name token when `key` carries trailing
/// inline arguments; `None` when the whole string is the name.
fn command_name_length(key: &str) -> Option<usize> {
    let len = key.find(char::is_whitespace)?;
    (len > 0).then_some(len)
}

/// A view name is stored in its canonical dotted spelling so that the
/// slash-separated form Laravel also accepts compares equal to it; every
/// other kind is stored as written.
fn normalised_key(kind: crate::symbol_map::LaravelStringKind, key: &str) -> String {
    if kind == crate::symbol_map::LaravelStringKind::View {
        crate::virtual_members::laravel::canonical_view_name(key).into_owned()
    } else {
        key.to_string()
    }
}

/// Attribute counterpart of [`try_emit_laravel_string_span`]: the key is the
/// argument named for `parameter`, else the first positional one, as in
/// `#[Config('app.timezone')]` and `#[RedirectToRoute(route: 'login')]`.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_string_span_partial_for_parameter(
    kind: crate::symbol_map::LaravelStringKind,
    argument_list: &PartialArgumentList<'_>,
    parameter: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(expr) = partial_argument_expr_for_parameter(argument_list, parameter) else {
        return;
    };
    push_laravel_string_span(kind, false, false, expr, content, spans);
}

/// Attribute counterpart of named parameter selection for config resources.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_config_resource_span_partial_for_parameter(
    resource: crate::symbol_map::LaravelConfigResource,
    access: crate::symbol_map::laravel_resources::ResourceAccess,
    argument_list: &PartialArgumentList<'_>,
    parameter: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(expr) = partial_argument_expr_for_parameter(argument_list, parameter) else {
        return;
    };
    push_laravel_string_span(
        crate::symbol_map::LaravelStringKind::ConfigResource(resource),
        access.is_write(),
        access.is_optional(),
        expr,
        content,
        spans,
    );
}
