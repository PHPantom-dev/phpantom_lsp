//! Config keys: the `config()` helper, the `Config` facade, and the config
//! repository the helper returns, which both read keys and write them.

use super::*;

/// Emit the config-key span for a call on the config repository, marking
/// `Config::set('…')` as a write: it declares the key it names, so the key
/// need not exist in any `config/*.php` file.
///
/// `getMany()` is the one member of the family that takes a list rather than
/// a key, so it names as many keys as the list holds.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_config_key_span(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    if member_name.eq_ignore_ascii_case("getMany") {
        try_emit_config_array_spans(argument_list, false, content, spans);
        return;
    }
    let is_write = member_name.eq_ignore_ascii_case("set");
    if is_write && try_emit_config_array_spans(argument_list, true, content, spans) {
        return;
    }
    emit_laravel_string_span(
        crate::symbol_map::StringKeyKind::Config,
        is_write,
        0,
        argument_list,
        content,
        spans,
    );
}

/// Emit the config-key spans for the `config()` helper, which both reads and
/// writes: `config('app.name')` names the key it reads, while
/// `config(['app.name' => 'Acme'])` declares every key it lists.
pub(in crate::symbol_map::extraction) fn try_emit_laravel_config_helper_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    if try_emit_config_array_spans(argument_list, true, content, spans) {
        return;
    }
    emit_laravel_string_span(
        crate::symbol_map::StringKeyKind::Config,
        false,
        0,
        argument_list,
        content,
        spans,
    );
}

/// The config keys `expr` writes when it is one of the calls a file's write
/// spans are read from: the array form of the `config()` helper, and `set()`
/// on the repository `config()` returns or on the `Config` facade.
///
/// The service-provider scan reads a provider's writes through here rather
/// than building the provider's whole symbol map, so a key a package
/// declares from its provider instead of a config file (`laravel/sanctum`
/// adds its guard this way) is judged by the same rules as a project's own.
pub(crate) fn config_keys_written_by(expr: &Expression<'_>, content: &str) -> Vec<String> {
    let mut spans = Vec::new();
    match expr {
        Expression::Call(Call::Function(call)) => {
            if let Expression::Identifier(ident) = call.function
                && strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("config")
            {
                try_emit_laravel_config_helper_spans(&call.argument_list, content, &mut spans);
            }
        }
        Expression::Call(Call::Method(call)) => {
            if let ClassLikeMemberSelector::Identifier(method) = &call.method
                && is_laravel_config_repository_call(call.object, bytes_to_str(method.value))
            {
                try_emit_laravel_config_key_span(
                    bytes_to_str(method.value),
                    &call.argument_list,
                    content,
                    &mut spans,
                );
            }
        }
        Expression::Call(Call::NullSafeMethod(call)) => {
            if let ClassLikeMemberSelector::Identifier(method) = &call.method
                && is_laravel_config_repository_call(call.object, bytes_to_str(method.value))
            {
                try_emit_laravel_config_key_span(
                    bytes_to_str(method.value),
                    &call.argument_list,
                    content,
                    &mut spans,
                );
            }
        }
        Expression::Call(Call::StaticMethod(call)) => {
            if let Expression::Identifier(class) = call.class
                && is_config_facade_name(strip_fqn_prefix(bytes_to_str(class.value())))
                && let ClassLikeMemberSelector::Identifier(method) = &call.method
                && is_config_repository_method(bytes_to_str(method.value))
            {
                try_emit_laravel_config_key_span(
                    bytes_to_str(method.value),
                    &call.argument_list,
                    content,
                    &mut spans,
                );
            }
        }
        _ => {}
    }
    spans
        .into_iter()
        .filter_map(|span| match span.kind {
            SymbolKind::StringKey {
                key,
                is_write: true,
                ..
            } => Some(key),
            _ => None,
        })
        .collect()
}

/// Whether a static call's class, as written, is the `Config` facade.
pub(in crate::symbol_map::extraction) fn is_config_facade_name(name: &str) -> bool {
    matches_laravel_facade(name, "Config")
}

/// Push a config-key span for every key the array argument of a
/// `set()`- or `getMany()`-shaped call names, reporting whether that
/// argument was an array at all so a caller can fall back to the
/// single-key spelling.
///
/// A `set()` array is written (`['app.name' => 'Acme']`), and only its key
/// side names a config path. A `getMany()` array is read and takes both
/// spellings the repository accepts: a bare entry is the key itself, and a
/// key/value entry names the key on the left with the default it falls back
/// to on the right.
fn try_emit_config_array_spans(
    argument_list: &ArgumentList<'_>,
    is_write: bool,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) -> bool {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return false;
    };
    let Some(elements) = crate::parser::array_literal_elements(first_arg.value()) else {
        return false;
    };
    for element in elements.iter() {
        let key = match element {
            ArrayElement::KeyValue(kv) => kv.key,
            // A bare entry is a key only where the array is read; on the
            // write side it carries no key to name.
            ArrayElement::Value(value) if !is_write => value.value,
            _ => continue,
        };
        push_laravel_string_span(
            crate::symbol_map::StringKeyKind::Config,
            is_write,
            false,
            key,
            content,
            spans,
        );
    }
    true
}

/// Returns `true` if `name` is a method on Laravel's `Repository` config
/// contract that accepts config keys — one as its first argument, or a list
/// of them in the case of `getMany()`.
pub(crate) fn is_config_repository_method(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "has"
            | "get"
            | "getmany"
            | "string"
            | "integer"
            | "float"
            | "boolean"
            | "array"
            | "collection"
            | "set"
            | "prepend"
            | "push"
    )
}

/// Returns `true` if `object` is a `config()` (or `\config()`) helper call and
/// `member_name` is a config-key-accepting method, e.g. `config()->get('app.name')`.
pub(in crate::symbol_map::extraction) fn is_laravel_config_repository_call(
    object: &Expression<'_>,
    member_name: &str,
) -> bool {
    is_config_repository_method(member_name) && is_config_helper_call(object)
}

/// Whether `object` is a call to the `config()` (or `\config()`) helper.
pub(in crate::symbol_map::extraction) fn is_config_helper_call(object: &Expression<'_>) -> bool {
    matches!(object, Expression::Call(Call::Function(func_call))
        if matches!(func_call.function, Expression::Identifier(ident)
            if strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("config")))
}
