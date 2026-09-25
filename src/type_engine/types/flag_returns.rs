//! Builtin return types that hinge on a single bit of a flags argument.
//!
//! `json_encode()` is declared `string|false`, but a call that passes
//! `JSON_THROW_ON_ERROR` can never return `false`: the failure it would have
//! reported that way is raised as a `JsonException` instead. Every call that
//! asks for the exception still gets told it may have been handed `false`,
//! which is one of the larger sources of spurious `string` argument
//! mismatches in code that opts into the exception.
//!
//! A conditional return type in [`crate::stub_patches`] cannot express this.
//! Its conditions name a type or a whole value, while the flag is one bit of
//! an integer that callers OR together freely, so `$flags is 4194304` would
//! miss every call that also asked for pretty printing. PHPStan ships
//! `JsonThrowOnErrorDynamicReturnTypeExtension` for the same reason.
//!
//! `json_decode()` needs nothing here. It is declared `mixed`, so it carries
//! no failure branch for the flag to remove.

use crate::php_type::{PhpType, TypeKind};
use crate::types::ParameterInfo;

use super::conditional::{ArgTypeResolver, split_text_args};

/// `JSON_THROW_ON_ERROR`, as defined by PHP's `ext/json/php_json.h`.
///
/// Spelled out rather than read back from the stubs because a numeric flags
/// argument has to be bit-tested against the value itself, and the constant is
/// part of PHP's stable ABI.
const JSON_THROW_ON_ERROR: i64 = 1 << 22;

/// Whether `func_name` is one of the functions [`flag_narrowed_return_type`]
/// can narrow.
///
/// Call sites consult this before gathering their argument text, so a call to
/// any other function pays a single name comparison.
pub(crate) fn has_flag_dependent_return(func_name: &str) -> bool {
    func_name.trim_start_matches('\\') == "json_encode"
}

/// The call's return type with the branch its flags argument rules out
/// removed, or `None` when the function has no flag-dependent return type or
/// the flag is not provably set.
///
/// `declared` is the function's own return type, so a stub that stops
/// declaring the branch (or a PHP version where it never existed) narrows to
/// nothing rather than to a type this module invented.
pub(crate) fn flag_narrowed_return_type(
    func_name: &str,
    params: &[ParameterInfo],
    text_args: &str,
    declared: &PhpType,
    arg_type_resolver: ArgTypeResolver<'_>,
) -> Option<PhpType> {
    if !has_flag_dependent_return(func_name) {
        return None;
    }
    let flags = bound_arg_text(params, text_args, "$flags")?;
    if !bitmask_sets_flag(
        &flags,
        "JSON_THROW_ON_ERROR",
        JSON_THROW_ON_ERROR,
        arg_type_resolver,
    ) {
        return None;
    }
    without_false(declared)
}

/// The source text bound to `param_name` at this call site, following PHP's
/// argument-binding rules so `flags:` as a named argument is found in
/// whichever position it was written.
fn bound_arg_text(params: &[ParameterInfo], text_args: &str, param_name: &str) -> Option<String> {
    let idx = params.iter().position(|p| p.name == param_name)?;
    let args = split_text_args(text_args);
    crate::call_args::bind_text_args_to_params(params, &args)
        .get(idx)
        .cloned()
        .flatten()
}

/// Whether the bitmask expression `text` provably sets `bit`.
///
/// Each `|`-separated term is tested on its own: by name (`JSON_THROW_ON_ERROR`,
/// with or without a leading `\`), as an integer literal, or through the
/// resolved type of any other expression when it turns out to be a literal
/// integer (a constant, or a variable holding one). A term that cannot be
/// pinned down is simply not a match, so an unknown mask leaves the declared
/// return type alone.
fn bitmask_sets_flag(
    text: &str,
    flag_name: &str,
    bit: i64,
    arg_type_resolver: ArgTypeResolver<'_>,
) -> bool {
    split_bitwise_or(text).into_iter().any(|term| {
        let term = term.trim();
        if term.trim_start_matches('\\') == flag_name {
            return true;
        }
        let value = crate::php_type::parse_php_int_literal(term).or_else(|| {
            arg_type_resolver
                .and_then(|resolve| resolve(term))
                .as_ref()
                .and_then(super::const_fold::literal_int_value)
        });
        value.is_some_and(|value| value & bit != 0)
    })
}

/// Split a bitmask expression into its top-level `|` operands.
///
/// Nested parentheses and bracket pairs are kept intact, and `||` is left
/// alone: it is a boolean operator, so an expression containing one is not a
/// mask this module can read apart.
fn split_bitwise_or(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth: u32 = 0;
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'|' if depth == 0 => {
                if bytes.get(i + 1) == Some(&b'|') {
                    return vec![text];
                }
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&text[start..]);
    parts
}

/// `ty` without its `false` member, or `None` when it has none to drop.
fn without_false(ty: &PhpType) -> Option<PhpType> {
    let TypeKind::Union(members) = ty.kind() else {
        return None;
    };
    let kept: Vec<PhpType> = members.iter().filter(|m| !m.is_false()).cloned().collect();
    if kept.len() == members.len() {
        return None;
    }
    match kept.len() {
        0 => None,
        1 => kept.into_iter().next(),
        _ => Some(PhpType::union(kept)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;

    fn json_encode_params() -> Vec<ParameterInfo> {
        ["$value", "$flags", "$depth"]
            .iter()
            .map(|name| ParameterInfo {
                name: atom(name),
                is_required: *name == "$value",
                type_hint: None,
                native_type_hint: None,
                description: None,
                default_value: None,
                is_variadic: false,
                is_reference: false,
                closure_this_type: None,
                param_out_type: None,
            })
            .collect()
    }

    fn narrow(text_args: &str) -> Option<String> {
        flag_narrowed_return_type(
            "json_encode",
            &json_encode_params(),
            text_args,
            &PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
            None,
        )
        .map(|ty| ty.to_string())
    }

    #[test]
    fn named_flag_drops_the_false_branch() {
        assert_eq!(
            narrow("$value, JSON_THROW_ON_ERROR").as_deref(),
            Some("string")
        );
    }

    #[test]
    fn flag_is_found_beside_other_flags() {
        assert_eq!(
            narrow("$value, JSON_PRETTY_PRINT | \\JSON_THROW_ON_ERROR").as_deref(),
            Some("string")
        );
    }

    #[test]
    fn numeric_mask_is_bit_tested() {
        assert_eq!(narrow("$value, 4194432").as_deref(), Some("string"));
        assert_eq!(narrow("$value, 128").as_deref(), None);
    }

    #[test]
    fn named_argument_is_bound_to_the_flags_parameter() {
        assert_eq!(
            narrow("$value, depth: 8, flags: JSON_THROW_ON_ERROR").as_deref(),
            Some("string")
        );
    }

    #[test]
    fn unknown_mask_leaves_the_declared_union() {
        assert_eq!(narrow("$value, $flags").as_deref(), None);
        assert_eq!(narrow("$value").as_deref(), None);
    }

    #[test]
    fn other_json_flags_leave_the_declared_union() {
        assert_eq!(narrow("$value, JSON_UNESCAPED_SLASHES").as_deref(), None);
    }

    #[test]
    fn resolved_literal_int_argument_is_bit_tested() {
        let resolver = |text: &str| match text {
            "self::FLAGS" => Some(PhpType::literal_int("4194304")),
            _ => None,
        };
        let narrowed = flag_narrowed_return_type(
            "json_encode",
            &json_encode_params(),
            "$value, self::FLAGS",
            &PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
            Some(&resolver),
        );
        assert_eq!(narrowed.map(|ty| ty.to_string()).as_deref(), Some("string"));
    }

    #[test]
    fn a_boolean_or_is_not_read_as_a_mask() {
        assert_eq!(split_bitwise_or("$a || JSON_THROW_ON_ERROR").len(), 1);
    }
}
