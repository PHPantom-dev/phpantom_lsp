//! Morph aliases: the keys a `Relation::morphMap()` registers and the types
//! the `whereHasMorph()` family maps back through it.

use super::*;
use crate::virtual_members::laravel::helpers::string_literal_at_range;

/// The morph-map API on `Illuminate\Database\Eloquent\Relations\Relation`
/// whose first argument holds the alias → model map.
pub(crate) fn is_morph_map_registration_method(member_name: &str) -> bool {
    member_name.eq_ignore_ascii_case("morphMap")
        || member_name.eq_ignore_ascii_case("enforceMorphMap")
}

/// The argument the `whereHasMorph()` family takes its morph types in.
pub(crate) const MORPH_TYPES_ARG: usize = 1;

/// Whether `member_name` is a query-builder method whose second argument is a
/// list of morph types (`whereHasMorph($relation, ['post', 'video'], …)`).
///
/// Laravel maps each entry through `Relation::getMorphedModel()`, so a plain
/// string there is a morph alias.
pub(crate) fn is_morph_types_query_method(member_name: &str) -> bool {
    [
        "hasMorph",
        "orHasMorph",
        "doesntHaveMorph",
        "orDoesntHaveMorph",
        "whereHasMorph",
        "orWhereHasMorph",
        "whereDoesntHaveMorph",
        "orWhereDoesntHaveMorph",
        "whereMorphRelation",
        "orWhereMorphRelation",
        "whereMorphDoesntHaveRelation",
        "orWhereMorphDoesntHaveRelation",
    ]
    .iter()
    .any(|method| member_name.eq_ignore_ascii_case(method))
}

/// Push a [`crate::symbol_map::LaravelStringKind::MorphAlias`] span for every
/// alias key of a `Relation::morphMap(['post' => Post::class, …])` argument.
pub(in crate::symbol_map::extraction) fn try_emit_morph_map_key_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Some(elements) = crate::parser::array_literal_elements(first_arg.value()) else {
        return;
    };
    for element in elements.iter() {
        if let ArrayElement::KeyValue(kv) = element {
            push_morph_alias_span(kv.key, content, spans);
        }
    }
}

/// Push a morph-alias span for the `$types` argument at `arg_index`, which
/// Laravel accepts as either a single string or an array of strings.
pub(in crate::symbol_map::extraction) fn try_emit_morph_type_spans(
    argument_list: &ArgumentList<'_>,
    arg_index: usize,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(arg) = argument_list.arguments.iter().nth(arg_index) else {
        return;
    };
    for_each_value(arg.value(), |expr| {
        push_morph_alias_span(expr, content, spans)
    });
}

/// Push a morph-alias span for a single string-literal expression, skipping the
/// spellings that are not aliases.
fn push_morph_alias_span(expr: &Expression<'_>, content: &str, spans: &mut Vec<SymbolSpan>) {
    let Some((alias, start, end)) = string_literal_at_range(expr, content) else {
        return;
    };
    // `'*'` is the wildcard the `whereHasMorph()` family accepts, and a value
    // containing a namespace separator is a class name rather than an alias
    // (both APIs take either form).
    if alias == "*" || alias.contains('\\') {
        return;
    }
    spans.push(SymbolSpan {
        start,
        end,
        kind: SymbolKind::LaravelStringKey {
            kind: crate::symbol_map::LaravelStringKind::MorphAlias,
            key: alias.to_string(),
            is_write: false,
            is_optional: false,
        },
    });
}
