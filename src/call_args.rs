//! Matching call-site arguments to function/method parameters.
//!
//! PHP lets a caller pass arguments positionally or by name, and named
//! arguments may appear in any order (and target parameters that
//! positional arguments skipped). Any code that needs to know "which
//! argument fills parameter X" — conditional return types, pass-by-reference
//! type seeding, argument-count checking — must resolve named arguments by
//! parameter name rather than by their ordinal position in the argument list.
//!
//! This module is the single place that encodes PHP's binding rules so the
//! various consumers stay consistent.

use mago_syntax::cst::{
    Argument, ArgumentList, Expression, NamedArgument, PartialArgument, PartialArgumentList,
};

use crate::atom::bytes_to_str;
use crate::types::ParameterInfo;

/// Strip the leading `$` from a parameter name (e.g. `"$text"` → `"text"`).
fn param_name_bare(param: &ParameterInfo) -> &str {
    let name = param.name.as_str();
    name.strip_prefix('$').unwrap_or(name)
}

/// For each parameter in `params`, find the call argument that supplies it.
///
/// PHP binding rules: positional arguments fill parameters left to right;
/// each named argument fills the parameter whose name matches (ignoring the
/// `$` prefix). Positional arguments beyond the last declared parameter bind
/// to a trailing variadic parameter when one exists. Arguments that match no
/// declared parameter (a named argument with an unknown name, or a positional
/// overflow with no variadic) are dropped.
///
/// Returns a vector parallel to `params`: entry `i` holds the expression
/// bound to parameter `i`, or `None` when no argument supplies it.
pub(crate) fn bind_args_to_params<'b>(
    params: &[ParameterInfo],
    argument_list: &ArgumentList<'b>,
) -> Vec<Option<&'b Expression<'b>>> {
    let mut bound: Vec<Option<&Expression>> = vec![None; params.len()];
    let variadic_idx = params.iter().position(|p| p.is_variadic);
    let mut next_positional = 0usize;

    for arg in argument_list.arguments.iter() {
        match arg {
            Argument::Positional(pos) => {
                let target = if next_positional < params.len() {
                    Some(next_positional)
                } else {
                    variadic_idx
                };
                if let Some(idx) = target
                    && bound[idx].is_none()
                {
                    bound[idx] = Some(pos.value);
                }
                next_positional += 1;
            }
            Argument::Named(named) => {
                let name = bytes_to_str(named.name.value);
                if let Some(idx) = params.iter().position(|p| param_name_bare(p) == name) {
                    bound[idx] = Some(named.value);
                }
            }
        }
    }

    bound
}

/// For each argument in `argument_list` (by call position), resolve the
/// declared parameter index in `params` that it binds to under PHP's
/// binding rules.
///
/// This is the call-position-to-declared-index counterpart of
/// [`bind_args_to_params`] (which goes the other way: declared index to
/// bound expression). Consumers that key off "the Nth argument in the
/// call" — e.g. closure-argument inference matching a `callable(...)`
/// type hint to the closure's own declared parameter — must translate
/// through this first, since a named argument can appear anywhere in the
/// call and reorder or skip parameters ahead of it.
///
/// Returns a vector parallel to `argument_list.arguments`: entry `i` holds
/// the declared parameter index bound to call-position `i`, or `None` when
/// the argument matches no declared parameter (an unknown named argument,
/// or a positional overflow with no variadic parameter).
pub(crate) fn resolve_declared_arg_indices(
    params: &[ParameterInfo],
    argument_list: &ArgumentList<'_>,
) -> Vec<Option<usize>> {
    let variadic_idx = params.iter().position(|p| p.is_variadic);
    let mut next_positional = 0usize;

    argument_list
        .arguments
        .iter()
        .map(|arg| match arg {
            Argument::Positional(_) => {
                let target = if next_positional < params.len() {
                    Some(next_positional)
                } else {
                    variadic_idx
                };
                next_positional += 1;
                target
            }
            Argument::Named(named) => {
                let name = bytes_to_str(named.name.value);
                params.iter().position(|p| param_name_bare(p) == name)
            }
        })
        .collect()
}

/// Split a textual argument into an optional parameter name and its value.
///
/// `"signature: Foo::class"` → `(Some("signature"), "Foo::class")`.
/// `"Foo::class"` → `(None, "Foo::class")`.
///
/// The named-argument `name:` prefix is distinguished from a `::` static
/// access and from a ternary `?:` by requiring a leading identifier followed
/// by a single `:` (not `::`). Both returned slices are trimmed.
fn split_named_text_arg(arg: &str) -> (Option<&str>, &str) {
    let trimmed = arg.trim();
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let name_end = i;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b':' && (i + 1 >= bytes.len() || bytes[i + 1] != b':') {
            let name = &trimmed[..name_end];
            let value = trimmed[i + 1..].trim();
            return (Some(name), value);
        }
    }
    (None, trimmed)
}

/// Return an argument's value text with any leading `name:` label removed.
pub(crate) fn text_arg_value(arg: &str) -> &str {
    split_named_text_arg(arg).1
}

/// The argument that binds a callee's first parameter, named `name`: the
/// first argument when it is positional, else the one labelled `name:`.
pub(crate) fn first_param_arg<'a, 'b>(
    argument_list: &'a ArgumentList<'b>,
    name: &str,
) -> Option<&'a Argument<'b>> {
    match argument_list.arguments.first()? {
        first @ Argument::Positional(_) => Some(first),
        Argument::Named(_) => argument_list.arguments.iter().find(
            |arg| matches!(arg, Argument::Named(named) if bytes_to_str(named.name.value) == name),
        ),
    }
}

/// The text-source counterpart of [`first_param_arg`]: the value text of the
/// argument binding a callee's first parameter, named `name`.
pub(crate) fn first_param_text_arg<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    let (label, value) = split_named_text_arg(args.first()?);
    if label.is_none() {
        return Some(value);
    }
    args.iter().find_map(|arg| match split_named_text_arg(arg) {
        (Some(label), value) if label == name => Some(value),
        _ => None,
    })
}

/// Whether a textual argument is written with a `name:` label.
pub(crate) fn text_arg_is_named(arg: &str) -> bool {
    split_named_text_arg(arg).0.is_some()
}

/// Bind already-split textual arguments to parameters by PHP's rules.
///
/// This is the text-source counterpart of [`bind_args_to_params`], used by
/// the conditional-return-type resolver that only has the raw argument text
/// available (not an AST `ArgumentList`). Positional arguments fill
/// parameters left to right; named arguments (`name: value`) fill the
/// parameter whose bare name matches. Returns a vector parallel to `params`
/// holding the trimmed value text bound to each parameter, or `None` when no
/// argument supplies it.
pub(crate) fn bind_text_args_to_params(
    params: &[ParameterInfo],
    text_args: &[&str],
) -> Vec<Option<String>> {
    let mut bound: Vec<Option<String>> = vec![None; params.len()];
    let variadic_idx = params.iter().position(|p| p.is_variadic);
    let mut next_positional = 0usize;

    for arg in text_args {
        let (name, value) = split_named_text_arg(arg);
        match name {
            None => {
                let target = if next_positional < params.len() {
                    Some(next_positional)
                } else {
                    variadic_idx
                };
                if let Some(idx) = target
                    && bound[idx].is_none()
                {
                    bound[idx] = Some(value.to_string());
                }
                next_positional += 1;
            }
            Some(n) => {
                if let Some(idx) = params.iter().position(|p| param_name_bare(p) == n) {
                    bound[idx] = Some(value.to_string());
                }
            }
        }
    }

    bound
}

/// One argument written in a partial application: a bound value (named or
/// positional), a `?` or `name: ?` placeholder, or the `...` placeholder.
enum PartialArg<N, V> {
    Bound(Option<N>, V),
    Placeholder(Option<N>),
    Variadic,
}

/// The arguments the wrapped call receives when a partial application with
/// `bound` arguments is invoked with the `call` arguments, as
/// `(name, value)` pairs: `$r->find(?, 1)(5)` calls `$r->find(5, 1)`.
///
/// Each `?` takes the next positional call argument and `name: ?` the call
/// argument of that name (else the next positional one); `...` takes every
/// call argument left over.  Once a `?` goes unfilled the positions after it
/// are unknown, so the bound positional arguments that follow are dropped
/// rather than shifted into its place.
fn bind_partial<N: Copy, V>(
    bound: impl Iterator<Item = PartialArg<N, V>>,
    call: impl Iterator<Item = (Option<N>, V)>,
    same_name: impl Fn(N, N) -> bool,
) -> Vec<(Option<N>, V)> {
    let mut positional = Vec::new();
    let mut named = Vec::new();
    for (name, value) in call {
        match name {
            Some(name) => named.push((name, value)),
            None => positional.push(value),
        }
    }
    let mut positional = positional.into_iter();
    let mut args = Vec::new();
    let mut positions_known = true;
    for arg in bound {
        match arg {
            PartialArg::Bound(None, value) => {
                if positions_known {
                    args.push((None, value));
                }
            }
            PartialArg::Bound(name @ Some(_), value) => args.push((name, value)),
            PartialArg::Placeholder(None) => match positional.next() {
                Some(value) if positions_known => args.push((None, value)),
                _ => positions_known = false,
            },
            PartialArg::Placeholder(Some(name)) => {
                let value = match named.iter().position(|&(n, _)| same_name(n, name)) {
                    Some(at) => Some(named.remove(at).1),
                    None => positional.next(),
                };
                if let Some(value) = value {
                    args.push((Some(name), value));
                }
            }
            PartialArg::Variadic => {
                if positions_known {
                    args.extend(positional.by_ref().map(|value| (None, value)));
                }
                args.extend(named.drain(..).map(|(name, value)| (Some(name), value)));
            }
        }
    }
    args
}

/// The arguments the call a partial application wraps receives when the
/// application (written with the `bound` arguments) is invoked with `call`.
/// See [`bind_partial`] for how placeholders are filled.
pub(crate) fn bind_partial_arguments<'a>(
    bound: &PartialArgumentList<'a>,
    call: &ArgumentList<'a>,
) -> Vec<Argument<'a>> {
    let bound_args = bound.arguments.iter().map(|arg| match arg {
        PartialArgument::Positional(arg) => {
            PartialArg::Bound(None, Argument::Positional(arg.clone()))
        }
        PartialArgument::Named(arg) => {
            PartialArg::Bound(Some(arg.name), Argument::Named(arg.clone()))
        }
        PartialArgument::Placeholder(_) => PartialArg::Placeholder(None),
        PartialArgument::NamedPlaceholder(arg) => PartialArg::Placeholder(Some(arg.name)),
        PartialArgument::VariadicPlaceholder(_) => PartialArg::Variadic,
    });
    let call_args = call.arguments.iter().map(|arg| match arg {
        Argument::Positional(_) => (None, arg.clone()),
        Argument::Named(named) => (Some(named.name), arg.clone()),
    });
    bind_partial(bound_args, call_args, |a, b| a.value == b.value)
        .into_iter()
        .map(|(name, arg)| match (name, arg) {
            // A positional call argument filling a `name: ?` placeholder.
            (Some(name), Argument::Positional(positional)) => Argument::Named(NamedArgument {
                name,
                colon: name.span,
                value: positional.value,
            }),
            (_, arg) => arg,
        })
        .collect()
}

/// [`bind_partial_arguments`] over argument text, for the subject extractor
/// that only has the source of the two argument lists: `bound` as written
/// between the partial application's parentheses and `call` as written
/// between the invoking call's.  Returns the combined argument text.
pub(crate) fn bind_partial_text_arguments(bound: &str, call: &str) -> String {
    let bound_args = crate::text_scan::split_top_level_commas(bound)
        .into_iter()
        .map(|arg| match split_named_text_arg(arg) {
            (None, "?") => PartialArg::Placeholder(None),
            (Some(name), "?") => PartialArg::Placeholder(Some(name)),
            (None, "...") => PartialArg::Variadic,
            (name, value) => PartialArg::Bound(name, value),
        });
    let call_args = crate::text_scan::split_top_level_commas(call)
        .into_iter()
        .map(split_named_text_arg);
    bind_partial(bound_args, call_args, |a, b| a == b)
        .into_iter()
        .map(|(name, value)| match name {
            Some(name) => format!("{name}: {value}"),
            None => value.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The parameters a partial application written with the `bound` argument
/// text leaves open, in the order a call to it fills them: each `?` and
/// `name: ?` placeholder's parameter as written, then for `...` every
/// parameter no argument names.  `$r->make(1, ?)` over `make(int $id,
/// string $class)` takes only `$class`.
pub(crate) fn partial_application_params(
    params: &[ParameterInfo],
    bound: &str,
) -> Vec<ParameterInfo> {
    let mut taken = vec![false; params.len()];
    let mut open = Vec::new();
    let mut position = 0;
    for arg in crate::text_scan::split_top_level_commas(bound) {
        let at = match split_named_text_arg(arg) {
            (None, "...") => {
                open.extend((0..params.len()).filter(|&i| !taken[i]));
                break;
            }
            (Some(name), _) => params.iter().position(|p| param_name_bare(p) == name),
            (None, _) => {
                let at = position.min(params.len().saturating_sub(1));
                position += 1;
                params
                    .get(at)
                    .filter(|p| at + 1 == position || p.is_variadic)
                    .map(|_| at)
            }
        };
        let Some(at) = at else { continue };
        if text_arg_value(arg) == "?" && !taken[at] {
            open.push(at);
        }
        taken[at] = true;
    }
    open.into_iter().map(|i| params[i].clone()).collect()
}

/// Names (with the `$` prefix) of required parameters that no argument
/// supplies, given `positional_count` positional arguments and the named
/// arguments in `named_arg_names` (parameter names without the `$` prefix).
///
/// Positional arguments fill the first `positional_count` parameters; named
/// arguments fill the parameter whose bare name matches. A required
/// parameter is satisfied when it is filled either way. The result is empty
/// when every required parameter is supplied.
pub(crate) fn missing_required_params<'a>(
    params: &[ParameterInfo],
    positional_count: u32,
    named_arg_names: impl Iterator<Item = &'a str> + Clone,
) -> Vec<String> {
    params
        .iter()
        .enumerate()
        .filter(|(idx, p)| {
            if !p.is_required {
                return false;
            }
            let filled_positionally = (*idx as u32) < positional_count;
            let filled_by_name = named_arg_names.clone().any(|n| n == param_name_bare(p));
            !filled_positionally && !filled_by_name
        })
        .map(|(_, p)| p.name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;

    fn param(name: &str, is_required: bool) -> ParameterInfo {
        ParameterInfo {
            name: atom(name),
            type_hint: None,
            native_type_hint: None,
            description: None,
            default_value: if is_required {
                None
            } else {
                Some("0".to_string())
            },
            is_required,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
            param_out_type: None,
            callable_invocation: None,
        }
    }

    #[test]
    fn text_arg_value_strips_only_named_argument_labels() {
        assert_eq!(text_arg_value("path: null"), "null");
        assert_eq!(text_arg_value("array: ['x:y']"), "['x:y']");
        assert_eq!(text_arg_value("Foo::class"), "Foo::class");
        assert_eq!(
            text_arg_value("$flag ? 'a:b' : 'c:d'"),
            "$flag ? 'a:b' : 'c:d'"
        );
        assert_eq!(text_arg_value("'key: value'"), "'key: value'");
    }

    #[test]
    fn named_arg_filling_optional_leaves_required_missing() {
        // function f(int $a, int $b = 0, int $c = 0) called as f(c: 3).
        let params = [param("$a", true), param("$b", false), param("$c", false)];
        let missing = missing_required_params(&params, 0, ["c"].into_iter());
        assert_eq!(missing, vec!["$a".to_string()]);
    }

    #[test]
    fn required_filled_by_name_is_satisfied() {
        let params = [param("$a", true), param("$b", false)];
        let missing = missing_required_params(&params, 0, ["a"].into_iter());
        assert!(missing.is_empty());
    }

    #[test]
    fn required_split_positional_and_named() {
        // f(int $a, int $b, int $c = 0) called as f(1, b: 2).
        let params = [param("$a", true), param("$b", true), param("$c", false)];
        let missing = missing_required_params(&params, 1, ["b"].into_iter());
        assert!(missing.is_empty());
    }

    #[test]
    fn multiple_required_reported_missing() {
        let params = [param("$a", true), param("$b", true), param("$c", false)];
        let missing = missing_required_params(&params, 0, ["c"].into_iter());
        assert_eq!(missing, vec!["$a".to_string(), "$b".to_string()]);
    }

    #[test]
    fn all_positional_satisfies_required() {
        let params = [param("$a", true), param("$b", true)];
        let missing = missing_required_params(&params, 2, std::iter::empty());
        assert!(missing.is_empty());
    }

    #[test]
    fn partial_application_params_keep_what_placeholders_leave_open() {
        let params = [param("$a", true), param("$b", true), param("$c", false)];
        let open = |bound: &str| -> Vec<String> {
            partial_application_params(&params, bound)
                .into_iter()
                .map(|p| p.name.to_string())
                .collect()
        };
        assert_eq!(open("1, ?"), ["$b"]);
        assert_eq!(open("?, 1"), ["$a"]);
        assert_eq!(open("c: ?, a: 1"), ["$c"]);
        assert_eq!(open("c: ?, ?"), ["$c", "$a"]);
        assert_eq!(open("?, ..."), ["$a", "$b", "$c"]);
        assert_eq!(open("b: 2, ..."), ["$a", "$c"]);
        assert_eq!(open("1, 2, 3, ?"), Vec::<String>::new());
    }

    #[test]
    fn partial_application_params_fill_a_variadic_once() {
        let mut ids = param("$ids", false);
        ids.is_variadic = true;
        let params = [param("$class", true), ids];
        let open: Vec<String> = partial_application_params(&params, "?, ?, ?")
            .into_iter()
            .map(|p| p.name.to_string())
            .collect();
        assert_eq!(open, ["$class", "$ids"]);
    }
}
