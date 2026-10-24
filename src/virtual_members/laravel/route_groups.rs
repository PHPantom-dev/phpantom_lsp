//! The name and URI prefixes a route group applies: read off the options
//! array of `Route::group([...])` and off the fluent `Route::prefix()`,
//! `name()`/`as()` chain in front of `->group()`.

use mago_syntax::cst::*;

use super::const_eval::{Scope, const_string_prefix};
use super::helpers::{
    extract_string_literal, extract_string_literal_including_empty, first_string_arg,
};

/// The value a string-keyed option (e.g. `'as' => 'admin.'`) is set to in a
/// `Route::group([…], fn(){})` argument list.
///
/// The array may be in any position; all non-array arguments are skipped.
fn group_option_expression<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
    option: &str,
) -> Option<&'a Expression<'a>> {
    for arg in args {
        let elements: Vec<&'a ArrayElement<'a>> = match arg {
            Expression::Array(arr) => arr.elements.iter().collect(),
            Expression::LegacyArray(arr) => arr.elements.iter().collect(),
            _ => continue,
        };
        for element in elements {
            let ArrayElement::KeyValue(kv) = element else {
                continue;
            };
            let Some((key, _, _)) = extract_string_literal(kv.key, content) else {
                continue;
            };
            if key == option {
                return Some(kv.value);
            }
        }
    }
    None
}

/// Extract a string-keyed option (e.g. `'as' => 'admin.'`) from a
/// `Route::group([…], fn(){})` argument list.
fn extract_group_option_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
    option: &str,
) -> String {
    group_option_expression(args, content, option)
        .and_then(|value| extract_string_literal(value, content))
        .map(|(value, _, _)| value.to_string())
        .unwrap_or_default()
}

/// Extract the `'as' => 'prefix.'` name prefix from a `Route::group([…], fn(){})` argument list.
pub(crate) fn extract_as_prefix_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
) -> String {
    extract_group_option_from_args(args, content, "as")
}

/// The statically-known head of an `'as' => …` group name that is not a plain
/// string literal, as [`chain_dynamic_name_prefix`] gives for the fluent
/// spelling of the same group.
///
/// `None` when the array names no prefix, or names one that is fully known.
pub(crate) fn dynamic_as_prefix_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
    scope: &Scope,
) -> Option<String> {
    let value = group_option_expression(args, content, "as")?;
    if extract_string_literal(value, content).is_some() {
        return None;
    }
    Some(const_string_prefix(value, content, scope))
}

/// Extract the `'prefix' => 'admin'` URI prefix from a `Route::group([…], fn(){})`
/// argument list.
pub(crate) fn extract_uri_prefix_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
) -> String {
    extract_group_option_from_args(args, content, "prefix")
}

/// Join two URI segments the way Laravel's route prefixing does: both sides
/// lose their surrounding slashes and are joined with a single `/`.
///
/// An empty side is dropped, so `join_uri_segments("admin", "")` is `"admin"`.
pub(crate) fn join_uri_segments(left: &str, right: &str) -> String {
    let left = left.trim_matches('/');
    let right = right.trim_matches('/');
    if left.is_empty() {
        right.to_string()
    } else if right.is_empty() {
        left.to_string()
    } else {
        format!("{left}/{right}")
    }
}

/// The value a call chain sets for one group modifier.
///
/// Walking `Route::name('admin.')->middleware(…)` with `methods = [b"name"]`
/// yields `"admin."`; the same walk with `[b"prefix"]` recovers the URI prefix
/// of `Route::prefix('admin')->…`.  `RouteRegistrar::attribute()` overwrites
/// rather than appends, so when the chain sets the attribute more than once the
/// outermost link (applied last) wins: `Route::name('a.')->name('b.')` groups
/// under `b.`.  A link whose argument is not a string literal sets a value we
/// cannot read, which yields the empty string.
fn chain_modifier_value(expr: &Expression<'_>, content: &str, methods: &[&[u8]]) -> String {
    let matches = |ident: &[u8]| methods.iter().any(|m| ident.eq_ignore_ascii_case(m));
    let (ident, args, object) = match expr {
        Expression::Call(Call::Method(mc)) => (&mc.method, &mc.argument_list, Some(mc.object)),
        // Route::name('prefix.') — static entry point of the chain.
        Expression::Call(Call::StaticMethod(sc)) => (&sc.method, &sc.argument_list, None),
        _ => return String::new(),
    };
    if let ClassLikeMemberSelector::Identifier(ident) = ident
        && matches(ident.value)
    {
        return first_string_arg(args, content).unwrap_or("").to_string();
    }
    match object {
        Some(object) => chain_modifier_value(object, content, methods),
        None => String::new(),
    }
}

/// The route-name prefix set by the call chain that precedes `->group()`.
///
/// `RouteRegistrar` aliases `->name()` onto `->as()`, so either spelling sets
/// it, and both the instance links and the static entry point
/// (`Route::name('prefix.')`) are read.
pub(crate) fn chain_name_prefix<'a>(expr: &Expression<'a>, content: &str) -> String {
    chain_modifier_value(expr, content, &[b"name", b"as"])
}

/// The statically-known head of the name prefix a call chain sets, when the
/// `->name(…)` that sets it is not a plain string literal.
///
/// `Route::name('filament.' . $panelId . '.')->group(…)` yields
/// `Some("filament.")`: the group's full prefix is incomplete, so the names it
/// registers cannot be enumerated, but they all start with the part that was
/// written out.  `None` means the chain's name is a literal (or never set) and
/// [`chain_name_prefix`] has the whole of it.
pub(crate) fn chain_dynamic_name_prefix(
    expr: &Expression<'_>,
    content: &str,
    scope: &Scope,
) -> Option<String> {
    let (ident, args, object) = match expr {
        Expression::Call(Call::Method(mc)) => (&mc.method, &mc.argument_list, Some(mc.object)),
        Expression::Call(Call::StaticMethod(sc)) => (&sc.method, &sc.argument_list, None),
        _ => return None,
    };
    if let ClassLikeMemberSelector::Identifier(ident) = ident
        && (ident.value.eq_ignore_ascii_case(b"name") || ident.value.eq_ignore_ascii_case(b"as"))
    {
        if first_string_arg(args, content).is_some() {
            return None;
        }
        return args
            .arguments
            .iter()
            .next()
            .map(|argument| const_string_prefix(argument.value(), content, scope));
    }
    chain_dynamic_name_prefix(object?, content, scope)
}

/// The `->prefix('...')` URI segment set by the call chain that precedes
/// `->group()`.
pub(crate) fn chain_uri_prefix<'a>(expr: &Expression<'a>, content: &str) -> String {
    chain_modifier_value(expr, content, &[b"prefix"])
}

/// The `->uri('...')` value a Folio mount chain sets
/// (`Folio::path(...)->uri('admin')`).
pub(crate) fn chain_uri_modifier<'a>(expr: &Expression<'a>, content: &str) -> String {
    chain_modifier_value(expr, content, &[b"uri"])
}

/// The route-name prefix a chain sets ahead of a `resource()` registration.
///
/// `RouteRegistrar` aliases `->name()` onto `->as()`, so both spellings feed
/// the same attribute, and unlike the group modifiers they *replace* rather
/// than accumulate: `Route::as('a')->as('b')->resource(…)` names its routes
/// `b.photos.index`.  `None` means the chain never set one, which is not the
/// same as setting it to the empty string.
pub(crate) fn chain_as_prefix<'a>(expr: &Expression<'a>, content: &str) -> Option<String> {
    let is_as =
        |ident: &[u8]| ident.eq_ignore_ascii_case(b"as") || ident.eq_ignore_ascii_case(b"name");
    let prefix = |args: &ArgumentList<'_>| {
        extract_string_literal_including_empty(args.arguments.first()?.value(), content)
            .map(|(value, _, _)| value.to_string())
    };
    match expr {
        Expression::Call(Call::Method(mc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return chain_as_prefix(mc.object, content);
            };
            // The outermost link is applied last, so it wins over anything
            // the rest of the chain set.
            if is_as(ident.value) {
                return prefix(&mc.argument_list);
            }
            chain_as_prefix(mc.object, content)
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return None;
            };
            is_as(ident.value)
                .then(|| prefix(&sc.argument_list))
                .flatten()
        }
        _ => None,
    }
}
