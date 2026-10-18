//! Macro names a `Macroable` class registers: `Str::macro('slug', …)`.

use super::*;
use crate::virtual_members::laravel::helpers::string_literal_at_range;

/// If `argument_list` starts with a plain, non-empty string literal, push a
/// [`SymbolKind::LaravelMacroString`] span covering the string content.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_macro_string_span(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some((name, start, end)) = argument_list
        .arguments
        .first()
        .and_then(|first_arg| string_literal_at_range(first_arg.value(), content))
    else {
        return;
    };
    spans.push(SymbolSpan {
        start,
        end,
        kind: SymbolKind::LaravelMacroString {
            name: name.to_string(),
        },
    });
}
