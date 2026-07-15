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

use mago_syntax::cst::{Argument, ArgumentList, Expression};

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

/// Names (with the `$` prefix) of required parameters that no argument
/// supplies, given `positional_count` positional arguments and the named
/// arguments in `named_arg_names` (parameter names without the `$` prefix).
///
/// Positional arguments fill the first `positional_count` parameters; named
/// arguments fill the parameter whose bare name matches. A required
/// parameter is satisfied when it is filled either way. The result is empty
/// when every required parameter is supplied.
pub(crate) fn missing_required_params(
    params: &[ParameterInfo],
    positional_count: u32,
    named_arg_names: &[String],
) -> Vec<String> {
    params
        .iter()
        .enumerate()
        .filter(|(idx, p)| {
            if !p.is_required {
                return false;
            }
            let filled_positionally = (*idx as u32) < positional_count;
            let filled_by_name = named_arg_names
                .iter()
                .any(|n| n.as_str() == param_name_bare(p));
            !filled_positionally && !filled_by_name
        })
        .map(|(_, p)| p.name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;

    /// Build a parameter with the given name and required-ness.
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
        }
    }

    #[test]
    fn named_arg_filling_optional_leaves_required_missing() {
        // function f(int $a, int $b = 0, int $c = 0) called as f(c: 3).
        let params = [param("$a", true), param("$b", false), param("$c", false)];
        let missing = missing_required_params(&params, 0, &["c".to_string()]);
        assert_eq!(missing, vec!["$a".to_string()]);
    }

    #[test]
    fn required_filled_by_name_is_satisfied() {
        let params = [param("$a", true), param("$b", false)];
        let missing = missing_required_params(&params, 0, &["a".to_string()]);
        assert!(missing.is_empty());
    }

    #[test]
    fn required_split_positional_and_named() {
        // f(int $a, int $b, int $c = 0) called as f(1, b: 2).
        let params = [param("$a", true), param("$b", true), param("$c", false)];
        let missing = missing_required_params(&params, 1, &["b".to_string()]);
        assert!(missing.is_empty());
    }

    #[test]
    fn multiple_required_reported_missing() {
        let params = [param("$a", true), param("$b", true), param("$c", false)];
        let missing = missing_required_params(&params, 0, &["c".to_string()]);
        assert_eq!(missing, vec!["$a".to_string(), "$b".to_string()]);
    }

    #[test]
    fn all_positional_satisfies_required() {
        let params = [param("$a", true), param("$b", true)];
        let missing = missing_required_params(&params, 2, &[]);
        assert!(missing.is_empty());
    }
}
