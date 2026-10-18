//! Builtin return types that the arguments of each call decide.
//!
//! A handful of builtins declare the widest result they can produce, while
//! the arguments of a given call pin it down far more tightly:
//!
//! - `json_encode()` is declared `string|false`, but a call that passes
//!   `JSON_THROW_ON_ERROR` can never return `false`: the failure it would
//!   have reported that way is raised as a `JsonException` instead.
//! - `filter_var()` and `filter_input()` are declared `mixed`, but the filter
//!   picks the result (`FILTER_VALIDATE_INT` yields an `int`, narrowed further
//!   by `min_range`/`max_range`) and `FILTER_NULL_ON_FAILURE` swaps the
//!   failure value from `false` to `null`.
//! - `class_implements()`, `class_parents()` and `class_uses()` only return
//!   `false` for a class that cannot be loaded. Handed an object they never
//!   do, and handed a class-string the failure is as unlikely as any other
//!   failed autoload, so the `false` is kept but marked benevolent.
//!
//! A conditional return type in [`crate::stub_patches`] cannot express these.
//! Its conditions name a type or a whole value, while a flag is one bit of an
//! integer that callers OR together freely (`$flags is 4194304` would miss
//! every call that also asked for pretty printing), and an options array is a
//! shape whose individual entries decide the result. PHPStan ships a dynamic
//! return type extension for each of these for the same reason.
//!
//! `json_decode()` needs nothing here. It is declared `mixed`, so it carries
//! no failure branch for the flag to remove.

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ParameterInfo;

use super::conditional::ArgTypeResolver;
use crate::text_scan::{ScanStep, scan_top_level, split_top_level_commas};

/// `JSON_THROW_ON_ERROR`, as defined by PHP's `ext/json/php_json.h`.
///
/// The constants in this module are spelled out rather than read back from
/// the stubs because a numeric flags argument has to be bit-tested against
/// the value itself, and the constants are part of PHP's stable ABI.
const JSON_THROW_ON_ERROR: i64 = 1 << 22;

/// `FILTER_DEFAULT` (also spelled `FILTER_UNSAFE_RAW`), the filter
/// `filter_var()` applies when none is given.
const FILTER_DEFAULT: i64 = 516;
const FILTER_VALIDATE_INT: i64 = 257;
const FILTER_VALIDATE_BOOL: i64 = 258;
const FILTER_VALIDATE_FLOAT: i64 = 259;
const FILTER_VALIDATE_REGEXP: i64 = 272;
const FILTER_VALIDATE_URL: i64 = 273;
const FILTER_VALIDATE_EMAIL: i64 = 274;
const FILTER_VALIDATE_IP: i64 = 275;
const FILTER_VALIDATE_MAC: i64 = 276;
const FILTER_VALIDATE_DOMAIN: i64 = 277;
const FILTER_REQUIRE_ARRAY: i64 = 16_777_216;
const FILTER_FORCE_ARRAY: i64 = 67_108_864;
const FILTER_NULL_ON_FAILURE: i64 = 134_217_728;
const FILTER_THROW_ON_FAILURE: i64 = 268_435_456;

/// Every `FILTER_*` constant a filter or flags argument may name, so a call
/// written with the constants reads without a round trip through the
/// constant resolver.
const FILTER_CONSTANTS: &[(&str, i64)] = &[
    ("FILTER_FLAG_NONE", 0),
    ("FILTER_FLAG_ALLOW_OCTAL", 1),
    ("FILTER_FLAG_ALLOW_HEX", 2),
    ("FILTER_FLAG_STRIP_LOW", 4),
    ("FILTER_FLAG_STRIP_HIGH", 8),
    ("FILTER_FLAG_ENCODE_LOW", 16),
    ("FILTER_FLAG_ENCODE_HIGH", 32),
    ("FILTER_FLAG_ENCODE_AMP", 64),
    ("FILTER_FLAG_NO_ENCODE_QUOTES", 128),
    ("FILTER_FLAG_EMPTY_STRING_NULL", 256),
    ("FILTER_FLAG_STRIP_BACKTICK", 512),
    ("FILTER_FLAG_ALLOW_FRACTION", 4096),
    ("FILTER_FLAG_ALLOW_THOUSAND", 8192),
    ("FILTER_FLAG_ALLOW_SCIENTIFIC", 16384),
    ("FILTER_FLAG_SCHEME_REQUIRED", 65536),
    ("FILTER_FLAG_HOST_REQUIRED", 131_072),
    ("FILTER_FLAG_PATH_REQUIRED", 262_144),
    ("FILTER_FLAG_QUERY_REQUIRED", 524_288),
    ("FILTER_FLAG_IPV4", 1_048_576),
    ("FILTER_FLAG_IPV6", 2_097_152),
    ("FILTER_FLAG_HOSTNAME", 1_048_576),
    ("FILTER_FLAG_EMAIL_UNICODE", 1_048_576),
    ("FILTER_FLAG_NO_RES_RANGE", 4_194_304),
    ("FILTER_FLAG_NO_PRIV_RANGE", 8_388_608),
    ("FILTER_FLAG_GLOBAL_RANGE", 536_870_912),
    ("FILTER_REQUIRE_SCALAR", 33_554_432),
    ("FILTER_REQUIRE_ARRAY", FILTER_REQUIRE_ARRAY),
    ("FILTER_FORCE_ARRAY", FILTER_FORCE_ARRAY),
    ("FILTER_NULL_ON_FAILURE", FILTER_NULL_ON_FAILURE),
    ("FILTER_THROW_ON_FAILURE", FILTER_THROW_ON_FAILURE),
    ("FILTER_VALIDATE_INT", FILTER_VALIDATE_INT),
    ("FILTER_VALIDATE_BOOLEAN", FILTER_VALIDATE_BOOL),
    ("FILTER_VALIDATE_BOOL", FILTER_VALIDATE_BOOL),
    ("FILTER_VALIDATE_FLOAT", FILTER_VALIDATE_FLOAT),
    ("FILTER_VALIDATE_REGEXP", FILTER_VALIDATE_REGEXP),
    ("FILTER_VALIDATE_DOMAIN", FILTER_VALIDATE_DOMAIN),
    ("FILTER_VALIDATE_URL", FILTER_VALIDATE_URL),
    ("FILTER_VALIDATE_EMAIL", FILTER_VALIDATE_EMAIL),
    ("FILTER_VALIDATE_IP", FILTER_VALIDATE_IP),
    ("FILTER_VALIDATE_MAC", FILTER_VALIDATE_MAC),
    ("FILTER_DEFAULT", FILTER_DEFAULT),
    ("FILTER_UNSAFE_RAW", FILTER_DEFAULT),
    ("FILTER_SANITIZE_STRING", 513),
    ("FILTER_SANITIZE_STRIPPED", 513),
    ("FILTER_SANITIZE_ENCODED", 514),
    ("FILTER_SANITIZE_SPECIAL_CHARS", 515),
    ("FILTER_SANITIZE_EMAIL", 517),
    ("FILTER_SANITIZE_URL", 518),
    ("FILTER_SANITIZE_NUMBER_INT", 519),
    ("FILTER_SANITIZE_NUMBER_FLOAT", 520),
    ("FILTER_SANITIZE_MAGIC_QUOTES", 521),
    ("FILTER_SANITIZE_FULL_SPECIAL_CHARS", 522),
    ("FILTER_SANITIZE_ADD_SLASHES", 523),
    ("FILTER_CALLBACK", 1024),
];

/// The functions [`call_site_return_type`] can narrow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallSiteFunction {
    JsonEncode,
    FilterVar,
    FilterInput,
    ClassRelation,
}

fn classify(func_name: &str) -> Option<CallSiteFunction> {
    let name = func_name.trim_start_matches('\\');
    if name.eq_ignore_ascii_case("json_encode") {
        Some(CallSiteFunction::JsonEncode)
    } else if name.eq_ignore_ascii_case("filter_var") {
        Some(CallSiteFunction::FilterVar)
    } else if name.eq_ignore_ascii_case("filter_input") {
        Some(CallSiteFunction::FilterInput)
    } else if ["class_implements", "class_parents", "class_uses"]
        .iter()
        .any(|f| name.eq_ignore_ascii_case(f))
    {
        Some(CallSiteFunction::ClassRelation)
    } else {
        None
    }
}

/// Whether `func_name` is one of the functions [`call_site_return_type`]
/// can narrow.
///
/// Call sites consult this before gathering their argument text, so a call to
/// any other function pays a few name comparisons.
pub(crate) fn has_call_site_return(func_name: &str) -> bool {
    classify(func_name).is_some()
}

/// The return type this call's arguments decide, or `None` when the function
/// is not one this module knows or the arguments cannot be pinned down.
///
/// `declared` is the function's own return type. Where the narrowing only
/// removes a branch (`json_encode()`, `class_implements()`), it is removed
/// from `declared`, so a stub that stops declaring the branch narrows to
/// nothing rather than to a type this module invented.
pub(crate) fn call_site_return_type(
    func_name: &str,
    params: &[ParameterInfo],
    text_args: &str,
    declared: &PhpType,
    arg_type_resolver: ArgTypeResolver<'_>,
) -> Option<PhpType> {
    let args = CallArgs {
        params,
        args: split_top_level_commas(text_args),
        resolver: arg_type_resolver,
    };
    match classify(func_name)? {
        CallSiteFunction::JsonEncode => json_encode_return(&args, declared),
        CallSiteFunction::FilterVar => filter_return(&args, "$filter", "$options"),
        CallSiteFunction::FilterInput => {
            // The variable may be missing from the input altogether, which
            // `filter_input()` reports as `null`, or as `false` when asked for
            // `null` on a failed filter.
            let filtered = filter_return(&args, "$filter", "$options")?;
            let missing = if filter_flags(&args, "$options")?.0 & FILTER_NULL_ON_FAILURE != 0 {
                PhpType::false_()
            } else {
                PhpType::null()
            };
            Some(PhpType::union(vec![filtered, missing]))
        }
        CallSiteFunction::ClassRelation => class_relation_return(&args, declared),
    }
}

/// One call's arguments, bound to the parameters they fill.
struct CallArgs<'a> {
    params: &'a [ParameterInfo],
    args: Vec<&'a str>,
    resolver: ArgTypeResolver<'a>,
}

impl CallArgs<'_> {
    /// The source text bound to `param_name` at this call site, following
    /// PHP's argument-binding rules so a named argument is found in whichever
    /// position it was written.
    fn text(&self, param_name: &str) -> Option<String> {
        let idx = self.params.iter().position(|p| p.name == param_name)?;
        crate::call_args::bind_text_args_to_params(self.params, &self.args)
            .get(idx)
            .cloned()
            .flatten()
    }

    fn resolve(&self, text: &str) -> Option<PhpType> {
        (self.resolver?)(text.trim())
    }
}

fn json_encode_return(args: &CallArgs<'_>, declared: &PhpType) -> Option<PhpType> {
    let flags = args.text("$flags")?;
    if !bitmask_sets_flag(
        &flags,
        "JSON_THROW_ON_ERROR",
        JSON_THROW_ON_ERROR,
        args.resolver,
    ) {
        return None;
    }
    without_false(declared)
}

/// `class_implements()` and its siblings, keyed on what the first argument
/// is: an object always has its class loaded, so `false` cannot happen; a
/// class-string only fails when autoloading does, which is the benevolent
/// kind of failure nobody checks for.
fn class_relation_return(args: &CallArgs<'_>, declared: &PhpType) -> Option<PhpType> {
    let subject = args.resolve(&args.text("$object_or_class")?)?;
    let subject = subject.strip_benevolence();
    let members: &[PhpType] = match subject.kind() {
        TypeKind::Union(members) => members,
        _ => std::slice::from_ref(&subject),
    };
    let mut all_objects = true;
    for member in members {
        if is_class_name_string(member) {
            all_objects = false;
        } else if !member.is_object_like() {
            return None;
        }
    }
    if all_objects {
        return without_false(declared);
    }
    // With autoloading switched off, a class-string naming a class nobody
    // has loaded yet really does produce `false`.
    let autoload_off = args
        .text("$autoload")
        .is_some_and(|text| text.trim().eq_ignore_ascii_case("false"));
    if autoload_off {
        return None;
    }
    let ty = PhpType::benevolent(declared.clone());
    ty.is_benevolent().then_some(ty)
}

/// Whether `ty` is a string known to name a class-like (`Foo::class`, a
/// `class-string<Foo>`, an `interface-string`, …).
fn is_class_name_string(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::ClassString(_) | TypeKind::InterfaceString(_) | TypeKind::ClassNameLiteral(_) => {
            true
        }
        TypeKind::Named(name) => [
            "class-string",
            "interface-string",
            "trait-string",
            "enum-string",
        ]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n)),
        _ => false,
    }
}

/// The flags a filter call was given, and the options array (when it has
/// one), read from `options_param`.
///
/// The argument is either a bitmask of flags or an array with `flags` and
/// `options` keys. `None` when it is neither readable form.
fn filter_flags(args: &CallArgs<'_>, options_param: &str) -> Option<(i64, Option<PhpType>)> {
    let Some(text) = args.text(options_param) else {
        return Some((0, None));
    };
    if let Some(mask) = mask_value(&text, args.resolver) {
        return Some((mask, None));
    }
    let resolved = args.resolve(&text)?;
    resolved.shape_entries()?;
    let flags = match resolved.shape_entry("flags") {
        Some(entry) if !entry.optional => literal_int(&entry.value_type)?,
        Some(_) => return None,
        None => 0,
    };
    let options = match resolved.shape_entry("options") {
        // An options entry that is not a readable shape could carry a
        // `default` that replaces the failure value; there is no telling
        // what the call returns then.
        Some(entry) if entry.value_type.shape_entries().is_some() && !entry.optional => {
            Some(entry.value_type.clone())
        }
        Some(_) => return None,
        None => None,
    };
    Some((flags, options))
}

/// What `filter_var()` returns for the filter and flags this call passes.
///
/// Mirrors PHPStan's `FilterFunctionReturnTypeHelper` minus the parts that
/// read the input value itself: the result is the type the filter produces,
/// plus the failure value (`false`, `null` under `FILTER_NULL_ON_FAILURE`,
/// or the `default` option) unless `FILTER_THROW_ON_FAILURE` turns failure
/// into an exception.
fn filter_return(args: &CallArgs<'_>, filter_param: &str, options_param: &str) -> Option<PhpType> {
    let filter = match args.text(filter_param) {
        Some(text) => mask_value(&text, args.resolver)?,
        None => FILTER_DEFAULT,
    };
    let (flags, options) = filter_flags(args, options_param)?;
    // Array input recurses through the filter element by element; the
    // result's shape follows the input's, which is not modelled here.
    if flags & (FILTER_REQUIRE_ARRAY | FILTER_FORCE_ARRAY) != 0 {
        return None;
    }

    let mut filtered = match filter {
        FILTER_VALIDATE_INT => PhpType::int(),
        FILTER_VALIDATE_BOOL => PhpType::bool(),
        FILTER_VALIDATE_FLOAT => PhpType::float(),
        FILTER_VALIDATE_URL | FILTER_VALIDATE_EMAIL | FILTER_VALIDATE_IP | FILTER_VALIDATE_MAC => {
            PhpType::named(atom("non-falsy-string"))
        }
        FILTER_VALIDATE_REGEXP | FILTER_VALIDATE_DOMAIN | FILTER_DEFAULT | 513..=523 => {
            PhpType::string()
        }
        // `FILTER_CALLBACK` returns whatever the callback does, and an
        // unknown filter id is a warning that returns `false`.
        _ => return None,
    };

    let option = |key: &str| {
        options
            .as_ref()
            .and_then(|o| o.shape_entry(key))
            .filter(|entry| !entry.optional)
            .map(|entry| entry.value_type.clone())
    };
    if filter == FILTER_VALIDATE_INT {
        let min = option("min_range").as_ref().and_then(literal_int);
        let max = option("max_range").as_ref().and_then(literal_int);
        if min.is_some() || max.is_some() {
            filtered = PhpType::int_range(
                min.map_or_else(|| "min".to_string(), |v| v.to_string()),
                max.map_or_else(|| "max".to_string(), |v| v.to_string()),
            );
        }
    }

    if flags & FILTER_THROW_ON_FAILURE != 0 {
        return Some(filtered);
    }
    let failure = option("default").unwrap_or_else(|| {
        if flags & FILTER_NULL_ON_FAILURE != 0 {
            PhpType::null()
        } else {
            PhpType::false_()
        }
    });
    // `bool|false` is just `bool`.
    if filtered.is_bool() && failure.is_false() {
        return Some(filtered);
    }
    Some(PhpType::union(vec![filtered, failure]))
}

/// The integer value of a bitmask expression, or `None` when any of its
/// `|`-separated terms cannot be pinned down.
///
/// Each term is read as a known `FILTER_*` constant (with or without a
/// leading `\`), an integer literal, or through the resolved type of any
/// other expression when that is a literal integer.
fn mask_value(text: &str, arg_type_resolver: ArgTypeResolver<'_>) -> Option<i64> {
    split_bitwise_or(text)
        .into_iter()
        .try_fold(0_i64, |acc, term| {
            Some(acc | term_value(term, arg_type_resolver)?)
        })
}

fn term_value(term: &str, arg_type_resolver: ArgTypeResolver<'_>) -> Option<i64> {
    let term = term.trim();
    let bare = term.trim_start_matches('\\');
    if let Some(&(_, value)) = FILTER_CONSTANTS.iter().find(|(name, _)| *name == bare) {
        return Some(value);
    }
    crate::php_type::parse_php_int_literal(term).or_else(|| {
        arg_type_resolver
            .and_then(|resolve| resolve(term))
            .as_ref()
            .and_then(literal_int)
    })
}

fn literal_int(ty: &PhpType) -> Option<i64> {
    super::const_fold::literal_int_value(ty)
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
                .and_then(literal_int)
        });
        value.is_some_and(|value| value & bit != 0)
    })
}

/// Split a bitmask expression into its top-level `|` operands.
///
/// Nested parentheses, bracket pairs, quotes and comments are kept intact
/// (see [`scan_top_level`]), and `||` is left alone: it is a boolean
/// operator, so an expression containing one is not a mask this module can
/// read apart.
fn split_bitwise_or(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    // Each pass restarts at depth zero just past a top-level `|`.
    while let Some(pipe) = scan_top_level(&bytes[start..], |bytes, i| {
        if bytes[i] == b'|' {
            ScanStep::Stop
        } else {
            ScanStep::Skip(1)
        }
    }) {
        let pipe = start + pipe;
        if bytes.get(pipe + 1) == Some(&b'|') {
            return vec![text];
        }
        parts.push(&text[start..pipe]);
        start = pipe + 1;
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
#[path = "call_site_returns_tests.rs"]
mod tests;
