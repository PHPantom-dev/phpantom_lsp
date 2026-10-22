use mago_syntax::cst::*;

mod resources;
use resources::*;
mod traversal;
pub(crate) use traversal::*;
mod discovery;
pub(crate) use discovery::*;

/// Whether `name` is one of the routes `pattern` names.
///
/// Most call sites write the name of exactly one route, and there `pattern`
/// is that name.  The "is the current route named …?" checks
/// (`Route::is('admin.*')`, `$request->routeIs('order.*')`) instead write a
/// pattern, which Laravel matches with `Str::is()`: `*` stands for any run of
/// characters and everything else is literal.
pub(crate) fn route_name_matches(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let mut segments = pattern.split('*').peekable();
    // Everything before the first `*` has to be exactly where it is written.
    let Some(mut rest) = segments.next().and_then(|head| name.strip_prefix(head)) else {
        return false;
    };
    while let Some(segment) = segments.next() {
        // Everything after the last `*` has to end the name; a pattern that
        // ends in one has an empty final segment, which every name ends with.
        if segments.peek().is_none() {
            return rest.ends_with(segment);
        }
        // A segment between two stars may start anywhere after the one
        // before it, and taking its earliest match is safe: a later one
        // only leaves less room for the segments that follow.
        let Some(index) = rest.find(segment) else {
            return false;
        };
        rest = &rest[index + segment.len()..];
    }
    true
}

pub(crate) use super::route_groups::{chain_name_prefix, extract_as_prefix_from_args};

#[cfg(test)]
mod tests;
