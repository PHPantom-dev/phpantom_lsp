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
//! `json_decode()` has no failure branch for the flag to remove, since it is
//! declared `mixed`, but a literal JSON argument decides the value it
//! decodes to. What both of them throw under the flag is decided here too
//! ([`call_site_throws`]).
//!
//! - `gettype()` and `get_debug_type()` are declared `string`, but each name
//!   their argument's runtime type from a fixed set, so an argument of known
//!   type decides which of those names comes back.
//! - `random_int()`, `rand()` and `mt_rand()` are declared `int`, but return
//!   a value between their bounds.
//! - `date()` is declared `string`, but a literal format decides which
//!   strings it can produce.
//! - `array_key_exists()` is declared `bool`, but a literal key and an array
//!   shape decide whether the key is there.
//!
//! WordPress functions whose arguments decide their result the same way are
//! in [`wordpress`].

use crate::atom::atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, ShapeEntry, TypeKind};
use crate::types::ParameterInfo;

use super::conditional::ArgTypeResolver;
use crate::text_scan::{ScanStep, scan_top_level, split_top_level_commas};
use crate::types::FunctionInfo;

mod array_search;
pub(crate) mod date_create;
mod mb_convert_encoding;
mod parent_class;
mod parse_url;
mod preg_split;
mod scanf;
mod wordpress;

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
    JsonDecode,
    FilterVar,
    FilterInput,
    ClassRelation,
    GetType,
    GetDebugType,
    CurlGetinfo,
    PregSplit,
    Random { swaps_bounds: bool },
    Date,
    ArrayKeyExists,
    ParseUrl,
    Scanf { stream: bool },
    ParentClass,
    ArraySearch,
    MbSubstituteCharacter,
    MbConvertEncoding,
    ArrayRand,
    DateFormat,
    DateCreate { from_format: bool },
    Strtotime,
    WordPress(wordpress::WordPressFunction),
}

fn classify(func_name: &str) -> Option<CallSiteFunction> {
    let name = func_name.trim_start_matches('\\');
    if name.eq_ignore_ascii_case("json_encode") {
        Some(CallSiteFunction::JsonEncode)
    } else if name.eq_ignore_ascii_case("json_decode") {
        Some(CallSiteFunction::JsonDecode)
    } else if name.eq_ignore_ascii_case("filter_var") {
        Some(CallSiteFunction::FilterVar)
    } else if name.eq_ignore_ascii_case("filter_input") {
        Some(CallSiteFunction::FilterInput)
    } else if ["class_implements", "class_parents", "class_uses"]
        .iter()
        .any(|f| name.eq_ignore_ascii_case(f))
    {
        Some(CallSiteFunction::ClassRelation)
    } else if name.eq_ignore_ascii_case("gettype") {
        Some(CallSiteFunction::GetType)
    } else if name.eq_ignore_ascii_case("get_debug_type") {
        Some(CallSiteFunction::GetDebugType)
    } else if name.eq_ignore_ascii_case("curl_getinfo") {
        Some(CallSiteFunction::CurlGetinfo)
    } else if name.eq_ignore_ascii_case("preg_split") {
        Some(CallSiteFunction::PregSplit)
    } else if name.eq_ignore_ascii_case("date") {
        Some(CallSiteFunction::Date)
    } else if name.eq_ignore_ascii_case("date_create")
        || name.eq_ignore_ascii_case("date_create_immutable")
    {
        Some(CallSiteFunction::DateCreate { from_format: false })
    } else if name.eq_ignore_ascii_case("date_create_from_format")
        || name.eq_ignore_ascii_case("date_create_immutable_from_format")
    {
        Some(CallSiteFunction::DateCreate { from_format: true })
    } else if name.eq_ignore_ascii_case("strtotime") {
        Some(CallSiteFunction::Strtotime)
    } else if name.eq_ignore_ascii_case("date_format") {
        Some(CallSiteFunction::DateFormat)
    } else if name.eq_ignore_ascii_case("array_key_exists")
        || name.eq_ignore_ascii_case("key_exists")
    {
        Some(CallSiteFunction::ArrayKeyExists)
    } else if name.eq_ignore_ascii_case("parse_url") {
        Some(CallSiteFunction::ParseUrl)
    } else if name.eq_ignore_ascii_case("sscanf") {
        Some(CallSiteFunction::Scanf { stream: false })
    } else if name.eq_ignore_ascii_case("fscanf") {
        Some(CallSiteFunction::Scanf { stream: true })
    } else if name.eq_ignore_ascii_case("get_parent_class") {
        Some(CallSiteFunction::ParentClass)
    } else if name.eq_ignore_ascii_case("array_search") {
        Some(CallSiteFunction::ArraySearch)
    } else if name.eq_ignore_ascii_case("array_rand") {
        Some(CallSiteFunction::ArrayRand)
    } else if name.eq_ignore_ascii_case("mb_convert_encoding") {
        Some(CallSiteFunction::MbConvertEncoding)
    } else if name.eq_ignore_ascii_case("mb_substitute_character") {
        Some(CallSiteFunction::MbSubstituteCharacter)
    } else if name.eq_ignore_ascii_case("random_int") {
        Some(CallSiteFunction::Random {
            swaps_bounds: false,
        })
    } else if name.eq_ignore_ascii_case("rand") || name.eq_ignore_ascii_case("mt_rand") {
        Some(CallSiteFunction::Random { swaps_bounds: true })
    } else {
        wordpress::classify(name).map(CallSiteFunction::WordPress)
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

/// The name [`call_site_return_type`] knows `function` by, or `None` when it
/// is not one of the functions it can narrow.
///
/// The functions are all global, so a namespaced function of the same short
/// name is not one of them.
pub(crate) fn call_site_name(function: &FunctionInfo) -> Option<&str> {
    let name = function.name.as_str();
    (function.namespace.is_none() && has_call_site_return(name)).then_some(name)
}

/// The return type this call's arguments decide, or `None` when the function
/// is not one this module knows or the arguments cannot be pinned down.
///
/// `declared` is the function's own return type, if it declares one. Where
/// the narrowing only removes a branch (`json_encode()`,
/// `class_implements()`), it is removed from `declared`, so a stub that
/// stops declaring the branch narrows to nothing rather than to a type this
/// module invented.
pub(crate) fn call_site_return_type(
    func_name: &str,
    params: &[ParameterInfo],
    text_args: &str,
    declared: Option<&PhpType>,
    arg_type_resolver: ArgTypeResolver<'_>,
    class_loader: Option<&ResolvedClassLoader<'_>>,
) -> Option<PhpType> {
    let args = CallArgs {
        params,
        args: split_top_level_commas(text_args),
        resolver: arg_type_resolver,
    };
    match classify(func_name)? {
        CallSiteFunction::JsonEncode => json_encode_return(&args, declared?),
        CallSiteFunction::JsonDecode => json_decode_return(&args),
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
        CallSiteFunction::ClassRelation => class_relation_return(&args, declared?),
        CallSiteFunction::GetType => type_name_return(&args, TypeNaming::GetType, class_loader),
        CallSiteFunction::GetDebugType => {
            type_name_return(&args, TypeNaming::GetDebugType, class_loader)
        }
        CallSiteFunction::CurlGetinfo => curl_getinfo_return(&args),
        CallSiteFunction::PregSplit => preg_split::return_type(&args),
        CallSiteFunction::Random { swaps_bounds } => random_return(&args, swaps_bounds),
        CallSiteFunction::Date => date_return(&args),
        CallSiteFunction::DateCreate { from_format } => {
            date_create::return_type(&args, declared?, from_format)
        }
        CallSiteFunction::Strtotime => date_create::strtotime(
            &args.resolve(&args.text("$datetime")?)?,
            args.text("$baseTimestamp")
                .is_some_and(|base| !base.trim().eq_ignore_ascii_case("null")),
        ),
        CallSiteFunction::DateFormat => {
            date_formatted(&args.resolve(&args.text("$format")?)?, false)
        }
        CallSiteFunction::ArrayKeyExists => array_key_exists_return(&args),
        CallSiteFunction::ParseUrl => parse_url::return_type(&args),
        CallSiteFunction::Scanf { stream } => scanf::return_type(&args, stream),
        CallSiteFunction::ParentClass => parent_class::return_type(&args, class_loader),
        CallSiteFunction::ArraySearch => array_search::return_type(&args),
        CallSiteFunction::MbSubstituteCharacter => mb_substitute_character_return(&args),
        CallSiteFunction::MbConvertEncoding => mb_convert_encoding::return_type(&args),
        CallSiteFunction::ArrayRand => array_rand_return(&args),
        CallSiteFunction::WordPress(function) => wordpress::return_type(function, &args),
    }
}

/// `array_key_exists()` of a literal key on an array shape: `true` when every
/// shape the array may be requires the key, `false` when none can hold it.
///
/// `None` when the key is not a literal `int` or `string`, or the array is
/// not a shape whose keys are all known.
fn array_key_exists_return(args: &CallArgs<'_>) -> Option<PhpType> {
    use crate::php_type::{LiteralValue, is_decimal_int_array_key, runtime_shape_keys};

    let key = args.resolve(&args.text("$key")?)?;
    let array = args.resolve(&args.text("$array")?)?;
    // Each key as the runtime key it is stored under: a decimal integer
    // string is the integer key it spells.
    let keys: Vec<String> = key
        .union_members()
        .into_iter()
        .map(|member| match member.as_literal()? {
            LiteralValue::Int(raw) => is_decimal_int_array_key(raw).then(|| raw.to_string()),
            literal @ LiteralValue::String(_) => Some(literal.string_content()?.into_owned()),
            LiteralValue::Float(_) => None,
        })
        .collect::<Option<_>>()?;
    let mut present = false;
    let mut absent = false;
    for member in array.union_members() {
        let parts = member.shape_parts()?;
        let shape_keys = runtime_shape_keys(parts.entries)?;
        // A class-constant key is stored as its spelling, not its value.
        if shape_keys.iter().any(|key| key.contains("::")) {
            return None;
        }
        for key in &keys {
            match shape_keys.iter().position(|shape_key| shape_key == key) {
                Some(index) if !parts.entries[index].optional => present = true,
                None if parts.tail.is_none() => absent = true,
                _ => return Some(PhpType::bool()),
            }
        }
    }
    Some(match (present, absent) {
        (true, false) => PhpType::true_(),
        (false, true) => PhpType::false_(),
        _ => PhpType::bool(),
    })
}

/// `array_rand($array, $num)`: one of the array's keys when picking one, and
/// a list of them when picking more. A `$num` that is not a literal may be
/// either.
fn array_rand_return(args: &CallArgs<'_>) -> Option<PhpType> {
    let array = args.resolve(&args.text("$array")?)?;
    let keys = array.iterable_key_type()?;
    if keys.is_never() {
        return None;
    }
    let picks = |n: i64| match n {
        1 => Some(keys.clone()),
        n if n > 1 => Some(PhpType::list(keys.clone())),
        // Fewer than one is a `ValueError`.
        _ => None,
    };
    let Some(text) = args.text("$num") else {
        return picks(1);
    };
    if let Some(n) = crate::php_type::parse_php_int_literal(text.trim()) {
        return picks(n);
    }
    // A count whose every value picks one key, or more than one, decides the
    // form even when it is not one literal (`mt_rand(3, 10)`).
    let interval = args
        .resolve(&text)
        .filter(|ty| ty.is_int_subtype())
        .and_then(|ty| super::int_interval::IntInterval::of_int(&ty));
    match interval {
        Some(interval) if interval.min == Some(1) && interval.max == Some(1) => picks(1),
        Some(interval) if interval.min.is_some_and(|min| min > 1) => picks(2),
        _ => Some(PhpType::union(vec![keys.clone(), PhpType::list(keys)])),
    }
}

/// `mb_substitute_character()`: without an argument (or with `null`) the
/// current setting, a mode name or a Unicode scalar value; with one, `true`
/// for a setting it accepts, since PHP 8 throws a `ValueError` for any other.
fn mb_substitute_character_return(args: &CallArgs<'_>) -> Option<PhpType> {
    use super::int_interval::IntInterval;
    const GETTER: &str = "'entity'|'long'|'none'|int<0, 55295>|int<57344, 1114111>";
    let Some(text) = args.text("$substitute_character") else {
        return Some(PhpType::parse(GETTER));
    };
    let subject = args.resolve(&text)?;
    let mut results: Vec<PhpType> = Vec::new();
    for member in subject.union_members() {
        let ty = if member.is_null() {
            PhpType::parse(GETTER)
        } else if member.is_object_like() || member.is_closure() {
            PhpType::never()
        } else if let Some(content) = member.as_literal().and_then(|l| {
            matches!(l, crate::php_type::LiteralValue::String(_))
                .then(|| l.string_content())
                .flatten()
        }) {
            let mode = content.to_ascii_lowercase();
            if ["none", "long", "entity"].contains(&mode.as_str()) {
                PhpType::true_()
            } else {
                PhpType::never()
            }
        } else if member.is_int_subtype() {
            let interval = IntInterval::of_int(member)?;
            let overlaps = |min: i64, max: i64| {
                interval.min.is_none_or(|low| low <= max)
                    && interval.max.is_none_or(|high| high >= min)
            };
            if overlaps(0, 0xD7FF) || overlaps(0xE000, 0x10_FFFF) {
                PhpType::true_()
            } else {
                PhpType::never()
            }
        } else {
            return None;
        };
        if !results.contains(&ty) {
            results.push(ty);
        }
    }
    Some(PhpType::union(results))
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

/// `JSON_OBJECT_AS_ARRAY`, as defined by PHP's `ext/json/php_json.h`.
const JSON_OBJECT_AS_ARRAY: i64 = 1;

/// `json_decode()` of a literal JSON string (or a union of them): the value
/// it decodes to, with objects as `stdClass` or, when `$associative` is
/// `true` or left `null` under `JSON_OBJECT_AS_ARRAY`, as array shapes.
/// Text that is not valid JSON decodes to `null`.
///
/// `None` when the JSON or the decoding mode cannot be read.
fn json_decode_return(args: &CallArgs<'_>) -> Option<PhpType> {
    let json = args.resolve(&args.text("$json")?)?;
    let texts: Vec<String> = json
        .union_members()
        .into_iter()
        .map(|member| Some(member.as_literal()?.string_content()?.into_owned()))
        .collect::<Option<_>>()?;
    let object_as_array = || {
        args.text("$flags").is_some_and(|flags| {
            bitmask_sets_flag(
                &flags,
                "JSON_OBJECT_AS_ARRAY",
                JSON_OBJECT_AS_ARRAY,
                args.resolver,
            )
        })
    };
    let associative = args.text("$associative");
    let modes: &[bool] = match associative.as_deref().map(str::trim) {
        None => &[false],
        Some(text) if text.eq_ignore_ascii_case("null") => {
            if object_as_array() {
                &[true]
            } else {
                &[false]
            }
        }
        Some(text) if text.eq_ignore_ascii_case("true") => &[true],
        Some(text) if text.eq_ignore_ascii_case("false") => &[false],
        Some(text) => {
            let ty = args.resolve(text)?;
            if ty.is_true() {
                &[true]
            } else if ty.is_false() {
                &[false]
            } else if ty.is_subtype_of(&PhpType::parse("bool|null")) {
                &[false, true]
            } else {
                return None;
            }
        }
    };
    let mut results: Vec<PhpType> = Vec::new();
    for text in &texts {
        let decoded = serde_json::from_str::<JsonValue>(text).ok();
        for &assoc in modes {
            let ty = match &decoded {
                Some(value) => json_value_type(value, assoc, &mut 0)?,
                None => PhpType::null(),
            };
            if !results.contains(&ty) {
                results.push(ty);
            }
        }
    }
    Some(PhpType::union(results))
}

/// How many values a decoded document may hold before it is left typed as
/// the declared `mixed`: a literal that large is a data blob, not a value a
/// caller tells apart.
const MAX_JSON_VALUES: usize = 64;

/// A decoded JSON value, keeping an object's keys in the order they were
/// written (PHP does too) rather than sorted.
enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl<'de> serde::Deserialize<'de> for JsonValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = JsonValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_unit<E>(self) -> Result<JsonValue, E> {
                Ok(JsonValue::Null)
            }
            fn visit_bool<E>(self, v: bool) -> Result<JsonValue, E> {
                Ok(JsonValue::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<JsonValue, E> {
                Ok(JsonValue::Int(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<JsonValue, E> {
                // PHP decodes an integer past `PHP_INT_MAX` as a float.
                Ok(i64::try_from(v).map_or(JsonValue::Float(v as f64), JsonValue::Int))
            }
            fn visit_f64<E>(self, v: f64) -> Result<JsonValue, E> {
                Ok(JsonValue::Float(v))
            }
            fn visit_str<E>(self, v: &str) -> Result<JsonValue, E> {
                Ok(JsonValue::String(v.to_string()))
            }
            fn visit_string<E>(self, v: String) -> Result<JsonValue, E> {
                Ok(JsonValue::String(v))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<JsonValue, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(JsonValue::Array(items))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<JsonValue, A::Error> {
                let mut entries: Vec<(String, JsonValue)> = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, JsonValue>()? {
                    // A repeated key keeps its first position and its last value.
                    match entries.iter_mut().find(|(k, _)| *k == key) {
                        Some(entry) => entry.1 = value,
                        None => entries.push((key, value)),
                    }
                }
                Ok(JsonValue::Object(entries))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// The PHP type `json_decode()` gives `value`, objects decoded as arrays
/// when `assoc`. `None` once `count` passes [`MAX_JSON_VALUES`].
fn json_value_type(value: &JsonValue, assoc: bool, count: &mut usize) -> Option<PhpType> {
    *count += 1;
    if *count > MAX_JSON_VALUES {
        return None;
    }
    Some(match value {
        JsonValue::Null => PhpType::null(),
        JsonValue::Bool(true) => PhpType::true_(),
        JsonValue::Bool(false) => PhpType::false_(),
        JsonValue::Int(v) => PhpType::literal_int(v.to_string()),
        JsonValue::Float(v) if v.is_finite() => PhpType::literal_float(format!("{v:?}")),
        JsonValue::Float(_) => PhpType::float(),
        JsonValue::String(v) => PhpType::literal_string_value(v),
        JsonValue::Array(items) => PhpType::array_shape(
            items
                .iter()
                .map(|item| {
                    Some(ShapeEntry {
                        key: None,
                        value_type: json_value_type(item, assoc, count)?,
                        optional: false,
                    })
                })
                .collect::<Option<_>>()?,
        ),
        JsonValue::Object(_) if !assoc => PhpType::named(atom("stdClass")),
        JsonValue::Object(entries) => PhpType::array_shape(
            entries
                .iter()
                .map(|(key, item)| {
                    let plain = crate::php_type::canonical_int_key(key).is_some()
                        || (key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
                    plain.then_some(())?;
                    Some(ShapeEntry {
                        key: Some(key.clone()),
                        value_type: json_value_type(item, assoc, count)?,
                        optional: false,
                    })
                })
                .collect::<Option<_>>()?,
        ),
    })
}

/// Whether the arguments of a call to `function` can decide what it throws
/// (see [`call_site_throws`]), so a caller only sets up an argument resolver
/// for the functions that need one.
pub(crate) fn has_call_site_throws(function: &FunctionInfo) -> bool {
    function.namespace.is_none()
        && (function.name.eq_ignore_ascii_case("json_decode")
            || function.name.eq_ignore_ascii_case("json_encode"))
}

/// The exceptions a call's arguments make it throw on top of what the
/// function declares, or `None` when they add nothing.
///
/// `json_decode()` and `json_encode()` throw `JsonException` only when their
/// flags set `JSON_THROW_ON_ERROR`, which no `@throws` tag on the stub can
/// say. Like PHPStan's `JsonThrowTypeExtension`, a flags argument that
/// cannot be pinned down adds nothing. The resolver is only consulted for a
/// flags term that is neither the constant's name nor an integer literal.
pub(crate) fn call_site_throws(
    function: &FunctionInfo,
    argument_list: &mago_syntax::cst::ArgumentList<'_>,
    content: &str,
    arg_type_resolver: ArgTypeResolver<'_>,
) -> Option<PhpType> {
    if !has_call_site_throws(function) {
        return None;
    }
    let texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        content,
    );
    let args: Vec<&str> = texts.iter().map(String::as_str).collect();
    let args = CallArgs {
        params: &function.parameters,
        args,
        resolver: arg_type_resolver,
    };
    let flags = args.text("$flags")?;
    bitmask_sets_flag(
        &flags,
        "JSON_THROW_ON_ERROR",
        JSON_THROW_ON_ERROR,
        arg_type_resolver,
    )
    .then(|| PhpType::named(atom("JsonException")))
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

/// Each `CURLINFO_*` option `curl_getinfo()` answers, its value, and the
/// type of what it returns for it. Mirrors PHPStan's
/// `CurlGetinfoFunctionDynamicReturnTypeExtension`.
const CURLINFO_OPTIONS: &[(&str, i64, &str)] = &[
    ("CURLINFO_EFFECTIVE_URL", 1_048_577, "string"),
    ("CURLINFO_FILETIME", 2_097_166, "int"),
    ("CURLINFO_TOTAL_TIME", 3_145_731, "float"),
    ("CURLINFO_NAMELOOKUP_TIME", 3_145_732, "float"),
    ("CURLINFO_CONNECT_TIME", 3_145_733, "float"),
    ("CURLINFO_PRETRANSFER_TIME", 3_145_734, "float"),
    ("CURLINFO_STARTTRANSFER_TIME", 3_145_745, "float"),
    ("CURLINFO_REDIRECT_COUNT", 2_097_172, "int"),
    ("CURLINFO_REDIRECT_TIME", 3_145_747, "float"),
    ("CURLINFO_REDIRECT_URL", 1_048_607, "string|false"),
    ("CURLINFO_PRIMARY_IP", 1_048_608, "string"),
    ("CURLINFO_PRIMARY_PORT", 2_097_192, "int"),
    ("CURLINFO_LOCAL_IP", 1_048_617, "string"),
    ("CURLINFO_LOCAL_PORT", 2_097_194, "int"),
    ("CURLINFO_SIZE_UPLOAD", 3_145_735, "float"),
    ("CURLINFO_SIZE_DOWNLOAD", 3_145_736, "float"),
    ("CURLINFO_SPEED_DOWNLOAD", 3_145_737, "float"),
    ("CURLINFO_SPEED_UPLOAD", 3_145_738, "float"),
    ("CURLINFO_HEADER_SIZE", 2_097_163, "int"),
    ("CURLINFO_HEADER_OUT", 2, "string|false"),
    ("CURLINFO_REQUEST_SIZE", 2_097_164, "int"),
    ("CURLINFO_SSL_VERIFYRESULT", 2_097_165, "int"),
    ("CURLINFO_CONTENT_LENGTH_DOWNLOAD", 3_145_743, "float"),
    ("CURLINFO_CONTENT_LENGTH_UPLOAD", 3_145_744, "float"),
    ("CURLINFO_CONTENT_TYPE", 1_048_594, "string|false"),
    ("CURLINFO_PRIVATE", 1_048_597, "mixed"),
    ("CURLINFO_RESPONSE_CODE", 2_097_154, "int"),
    ("CURLINFO_HTTP_CODE", 2_097_154, "int"),
    ("CURLINFO_HTTP_CONNECTCODE", 2_097_174, "int"),
    ("CURLINFO_HTTPAUTH_AVAIL", 2_097_175, "int"),
    ("CURLINFO_PROXYAUTH_AVAIL", 2_097_176, "int"),
    ("CURLINFO_OS_ERRNO", 2_097_177, "int"),
    ("CURLINFO_NUM_CONNECTS", 2_097_178, "int"),
    ("CURLINFO_SSL_ENGINES", 4_194_331, "list<string>"),
    ("CURLINFO_COOKIELIST", 4_194_332, "list<string>"),
    ("CURLINFO_FTP_ENTRY_PATH", 1_048_606, "string|false"),
    ("CURLINFO_APPCONNECT_TIME", 3_145_761, "float"),
    (
        "CURLINFO_CERTINFO",
        4_194_338,
        "list<array<string, string>>",
    ),
    ("CURLINFO_CONDITION_UNMET", 2_097_187, "int"),
    ("CURLINFO_RTSP_CLIENT_CSEQ", 2_097_189, "int"),
    ("CURLINFO_RTSP_CSEQ_RECV", 2_097_191, "int"),
    ("CURLINFO_RTSP_SERVER_CSEQ", 2_097_190, "int"),
    ("CURLINFO_RTSP_SESSION_ID", 1_048_612, "string|false"),
    ("CURLINFO_HTTP_VERSION", 2_097_198, "int"),
    ("CURLINFO_PROTOCOL", 2_097_200, "int"),
    ("CURLINFO_PROXY_SSL_VERIFYRESULT", 2_097_199, "int"),
    ("CURLINFO_SCHEME", 1_048_625, "string"),
    ("CURLINFO_CONTENT_LENGTH_DOWNLOAD_T", 6_291_471, "int"),
    ("CURLINFO_CONTENT_LENGTH_UPLOAD_T", 6_291_472, "int"),
    ("CURLINFO_SIZE_DOWNLOAD_T", 6_291_464, "int"),
    ("CURLINFO_SIZE_UPLOAD_T", 6_291_463, "int"),
    ("CURLINFO_SPEED_DOWNLOAD_T", 6_291_465, "int"),
    ("CURLINFO_SPEED_UPLOAD_T", 6_291_466, "int"),
    ("CURLINFO_APPCONNECT_TIME_T", 6_291_512, "int"),
    ("CURLINFO_CONNECT_TIME_T", 6_291_508, "int"),
    ("CURLINFO_FILETIME_T", 6_291_470, "int"),
    ("CURLINFO_NAMELOOKUP_TIME_T", 6_291_507, "int"),
    ("CURLINFO_PRETRANSFER_TIME_T", 6_291_509, "int"),
    ("CURLINFO_REDIRECT_TIME_T", 6_291_511, "int"),
    ("CURLINFO_STARTTRANSFER_TIME_T", 6_291_510, "int"),
    ("CURLINFO_TOTAL_TIME_T", 6_291_506, "int"),
    ("CURLINFO_EFFECTIVE_METHOD", 1_048_634, "string"),
    ("CURLINFO_PROXY_ERROR", 2_097_211, "int"),
    ("CURLINFO_REFERER", 1_048_636, "string|false"),
    ("CURLINFO_RETRY_AFTER", 6_291_513, "int"),
    ("CURLINFO_CAINFO", 1_048_637, "string|false"),
    ("CURLINFO_CAPATH", 1_048_638, "string|false"),
    ("CURLINFO_POSTTRANSFER_TIME_T", 6_291_523, "int|false"),
    ("CURLINFO_QUEUE_TIME_T", 6_291_521, "int|false"),
    ("CURLINFO_USED_PROXY", 2_097_218, "int|false"),
    ("CURLINFO_HTTPAUTH_USED", 2_097_221, "int|false"),
    ("CURLINFO_PROXYAUTH_USED", 2_097_222, "int|false"),
    ("CURLINFO_CONN_ID", 6_291_520, "int|false"),
];

/// What `curl_getinfo()` returns without an option: every piece of
/// information in one array, or `false` on failure.
const CURL_GETINFO_ALL: &str = "array{url: string, content_type: string|null, http_code: int, \
    header_size: int, request_size: int, filetime: int, ssl_verify_result: int, \
    redirect_count: int, total_time: float, namelookup_time: float, connect_time: float, \
    pretransfer_time: float, size_upload: float, size_download: float, speed_download: float, \
    speed_upload: float, download_content_length: float, upload_content_length: float, \
    starttransfer_time: float, redirect_time: float, redirect_url: string, primary_ip: string, \
    certinfo: list<array<string, string>>, primary_port: int, local_ip: string, \
    local_port: int, http_version: int, protocol: int, ssl_verifyresult: int, scheme: string, \
    appconnect_time_us: int, queue_time_us: int, connect_time_us: int, \
    namelookup_time_us: int, pretransfer_time_us: int, redirect_time_us: int, \
    starttransfer_time_us: int, posttransfer_time_us: int, total_time_us: int, \
    request_header: string, effective_method: string, capath: string, cainfo: string, \
    used_proxy: int, httpauth_used: int, proxyauth_used: int, conn_id: int}";

/// `curl_getinfo($handle, $option)`: the type the `CURLINFO_*` option
/// names, `false` for an integer that names none, and the array of every
/// piece of information (benevolently `false` on failure, which a handle
/// that made its request does not hit) without an option.
///
/// `None` for an option that cannot be pinned to one integer.
fn curl_getinfo_return(args: &CallArgs<'_>) -> Option<PhpType> {
    args.text("$handle")?;
    let option = match args.text("$option") {
        Some(text) if !text.trim().eq_ignore_ascii_case("null") => text,
        _ => {
            return Some(PhpType::benevolent(PhpType::union(vec![
                PhpType::parse(CURL_GETINFO_ALL),
                PhpType::false_(),
            ])));
        }
    };
    let option = option.trim();
    let bare = option.trim_start_matches('\\');
    if let Some(&(_, _, ty)) = CURLINFO_OPTIONS.iter().find(|(name, _, _)| *name == bare) {
        return Some(PhpType::parse(ty));
    }
    let value = crate::php_type::parse_php_int_literal(option)
        .or_else(|| args.resolve(option).as_ref().and_then(literal_int))?;
    Some(
        CURLINFO_OPTIONS
            .iter()
            .find(|(_, v, _)| *v == value)
            .map_or_else(PhpType::false_, |&(_, _, ty)| PhpType::parse(ty)),
    )
}

/// `random_int($min, $max)`, `rand($min, $max)` and `mt_rand($min, $max)`:
/// the range from the lowest value `$min` may hold to the highest `$max`
/// may. Without arguments `rand()` and `mt_rand()` return a non-negative
/// int. Mirrors PHPStan's `RandomIntFunctionReturnTypeExtension`.
///
/// `rand()` and `mt_rand()` accept their bounds either way round, where
/// `random_int()` throws for a `$max` below `$min`; `swaps_bounds` says
/// which of the two a call is to.
fn random_return(args: &CallArgs<'_>, swaps_bounds: bool) -> Option<PhpType> {
    if args.args.iter().all(|a| a.trim().is_empty()) {
        return swaps_bounds.then(|| PhpType::int_range("0", "max"));
    }
    let bound = |param: &str| {
        let ty = args.resolve(&args.text(param)?)?;
        let intervals = ty
            .union_members()
            .into_iter()
            .map(super::int_interval::IntInterval::of_int)
            .collect::<Option<Vec<_>>>()?;
        Some(super::int_interval::IntInterval::hull_of(&intervals))
    };
    let (low, high) = (bound("$min")?, bound("$max")?);
    let (min, max) = (low.min, high.max);
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        if !swaps_bounds {
            return None;
        }
        return Some(
            super::int_interval::IntInterval {
                min: Some(max),
                max: Some(min),
            }
            .into_type(),
        );
    }
    Some(super::int_interval::IntInterval { min, max }.into_type())
}

/// The characters `date()` replaces in its format, every other character
/// being copied as written.
const DATE_FORMAT_CHARS: &str = "dDjlNSwzWFmMntLoXxYyaABgGhHisuveIOPpTZcrU";

/// The format characters whose output is all digits.
const DATE_DIGIT_CHARS: &str = "djNwzWmntLoYygGhHisuvBUI";

/// `date($format)` over a literal format (or a union of them): a format of
/// one character that picks a number gives every string it can be, one
/// with no format characters is copied as written, and any other gives a
/// `numeric-string` when every character it outputs is a digit and a
/// `non-falsy-string` otherwise. Mirrors PHPStan's
/// `DateFunctionReturnTypeHelper`.
fn date_return(args: &CallArgs<'_>) -> Option<PhpType> {
    date_formatted(&args.resolve(&args.text("$format")?)?, true)
}

/// What formatting a date with `format` gives, when it is a literal or a
/// union of them; see [`date_return`].
///
/// `whole_seconds` is true for `date()`, whose `u` and `v` are always zero,
/// and false for a `DateTimeInterface`, which keeps its microseconds.
pub(crate) fn date_formatted(format: &PhpType, whole_seconds: bool) -> Option<PhpType> {
    let formats = literal_strings(format)?;
    let mut results: Vec<PhpType> = Vec::new();
    for format in &formats {
        let ty = date_format_type(format, whole_seconds);
        if !results.contains(&ty) {
            results.push(ty);
        }
    }
    (!results.is_empty()).then(|| PhpType::union(results))
}

fn date_format_type(format: &str, whole_seconds: bool) -> PhpType {
    let numbers = |min: u32, max: u32, pad: bool| {
        PhpType::union(
            (min..=max)
                .map(|i| {
                    PhpType::literal_string_value(&if pad {
                        format!("{i:02}")
                    } else {
                        i.to_string()
                    })
                })
                .collect(),
        )
    };
    match format {
        "d" => return numbers(1, 31, true),
        "j" => return numbers(1, 31, false),
        "N" => return numbers(1, 7, false),
        "w" => return numbers(0, 6, false),
        "m" => return numbers(1, 12, true),
        "n" | "g" => return numbers(1, 12, false),
        "t" => return numbers(28, 31, false),
        "L" | "I" => return numbers(0, 1, false),
        "G" => return numbers(0, 23, false),
        "h" => return numbers(1, 12, true),
        "H" => return numbers(0, 23, true),
        // `date()` works on whole seconds.
        "u" if whole_seconds => return PhpType::literal_string_value("000000"),
        "v" if whole_seconds => return PhpType::literal_string_value("000"),
        // A fixed number of digits, so never the falsy `'0'`.
        "u" | "v" => return PhpType::parse("non-falsy-string&numeric-string"),
        _ => {}
    }
    let mut copied = String::new();
    let mut has_format_char = false;
    let mut all_digits = !format.is_empty();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // An escaped character is copied as written.
            if let Some(next) = chars.next() {
                copied.push(next);
                all_digits &= next.is_ascii_digit();
            }
        } else if DATE_FORMAT_CHARS.contains(c) {
            has_format_char = true;
            all_digits &= DATE_DIGIT_CHARS.contains(c);
        } else {
            copied.push(c);
            all_digits &= c.is_ascii_digit();
        }
    }
    if !has_format_char {
        return PhpType::literal_string_value(&copied);
    }
    PhpType::named(atom(if all_digits {
        "numeric-string"
    } else {
        "non-falsy-string"
    }))
}

/// Which of PHP's two type-naming functions a call is to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeNaming {
    GetType,
    GetDebugType,
}

/// Every name `gettype()` can return, for an argument of unknown type.
const GETTYPE_NAMES: [&str; 10] = [
    "array",
    "boolean",
    "double",
    "integer",
    "NULL",
    "object",
    "resource",
    "resource (closed)",
    "string",
    "unknown type",
];

/// `gettype()` and `get_debug_type()`: the names their argument's type
/// decides, one literal per member of a union.
///
/// `get_debug_type()` names an object by its class, which only a final
/// class pins down (a subclass instance would report its own name), so any
/// other object, or a resource (named after its resource type), leaves the
/// declared `string`. Mirrors PHPStan's `GettypeFunctionReturnTypeExtension`
/// and `GetDebugTypeFunctionReturnTypeExtension`.
fn type_name_return(
    args: &CallArgs<'_>,
    naming: TypeNaming,
    class_loader: Option<&ResolvedClassLoader<'_>>,
) -> Option<PhpType> {
    let ty = args.resolve(&args.text("$value")?)?.strip_benevolence();
    let mut members = Vec::new();
    flatten_union(&ty, &mut members);
    let mut names: Vec<String> = Vec::new();
    for member in &members {
        for name in type_names(member, naming, class_loader)? {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    Some(PhpType::union(
        names.iter().map(PhpType::literal_string_value).collect(),
    ))
}

fn flatten_union(ty: &PhpType, out: &mut Vec<PhpType>) {
    match ty.kind() {
        TypeKind::Union(members) => members.iter().for_each(|m| flatten_union(m, out)),
        TypeKind::Nullable(inner) => {
            flatten_union(inner, out);
            out.push(PhpType::null());
        }
        _ => out.push(ty.clone()),
    }
}

/// The names one non-union type member is reported as, or `None` when they
/// cannot be told (an object `get_debug_type()` cannot name, a type this
/// does not recognise).
fn type_names(
    ty: &PhpType,
    naming: TypeNaming,
    class_loader: Option<&ResolvedClassLoader<'_>>,
) -> Option<Vec<String>> {
    let debug = naming == TypeNaming::GetDebugType;
    let one = |gettype: &str, debug_type: &str| {
        Some(vec![if debug { debug_type } else { gettype }.to_string()])
    };
    if ty.is_null() {
        return one("NULL", "null");
    }
    if ty.is_bool() || ty.is_true() || ty.is_false() {
        return one("boolean", "bool");
    }
    if ty.is_int_subtype() {
        return one("integer", "int");
    }
    if ty.is_float() || ty.is_float_subtype() {
        return one("double", "float");
    }
    if ty.is_string_subtype() || is_class_name_string(ty) {
        return one("string", "string");
    }
    if ty.is_array_key() {
        let names = if debug {
            ["int", "string"]
        } else {
            ["integer", "string"]
        };
        return Some(names.iter().map(|n| n.to_string()).collect());
    }
    if ty.is_array_like() && !ty.is_iterable() {
        return one("array", "array");
    }
    if ty.is_resource() {
        return (!debug).then(|| vec!["resource".to_string(), "resource (closed)".to_string()]);
    }
    if ty.is_mixed() {
        return (!debug).then(|| GETTYPE_NAMES.iter().map(|n| n.to_string()).collect());
    }
    if let TypeKind::Named(name) = ty.kind()
        && name.eq_ignore_ascii_case("object")
    {
        return (!debug).then(|| vec!["object".to_string()]);
    }
    // A class: confirmed by loading it, since an unresolved name (a
    // template parameter, a missing class) says nothing about the value.
    let class_name = match ty.kind() {
        TypeKind::Generic(g) => g.name.as_str(),
        _ => ty.base_name()?,
    };
    let class = class_loader?(Fqn::new(class_name.trim_start_matches('\\')))?;
    if !debug {
        return one("object", "object");
    }
    // An anonymous class cannot be extended, so it is the object's exact
    // class. PHP names it after its parent, else its first interface.
    if class.name.starts_with(crate::types::ANONYMOUS_CLASS_PREFIX) {
        let base = class
            .parent_class
            .or_else(|| class.interfaces.first().copied());
        let base = base
            .as_deref()
            .map_or("class", |b| b.trim_start_matches('\\'));
        return Some(vec![format!("{base}@anonymous")]);
    }
    let fqn = class.fqn();
    (class.is_final && !fqn.contains('@')).then(|| vec![fqn.to_string()])
}

/// Whether `ty` is a string known to name a class-like (`Foo::class`, a
/// `class-string<Foo>`, an `interface-string`, …).
fn is_class_name_string(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::ClassString(_)
        | TypeKind::InterfaceString(_)
        | TypeKind::ClassNameLiteral(..) => true,
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
    if options.is_none()
        && let Some(known) = filter_known_input_return(args, filter, flags)
    {
        return Some(known);
    }
    let element = filter_scalar_return(filter, flags, options.as_ref())?;
    // Array input runs through the filter entry by entry, each failure
    // reported in its place. `FILTER_FORCE_ARRAY` wraps a scalar input in an
    // array, where `FILTER_REQUIRE_ARRAY` fails on one.
    if flags & FILTER_FORCE_ARRAY != 0 {
        return Some(PhpType::generic_array_val(element));
    }
    if flags & FILTER_REQUIRE_ARRAY != 0 {
        let array = PhpType::generic_array_val(element);
        if flags & FILTER_THROW_ON_FAILURE != 0 {
            return Some(array);
        }
        return Some(PhpType::union(vec![
            array,
            filter_failure(flags, options.as_ref()),
        ]));
    }
    Some(element)
}

/// `filter_var()` of an input whose type decides what the filter does with
/// it: an `int` passes `FILTER_VALIDATE_INT` unchanged, `'12'` becomes `12`,
/// `null` always fails, and so on, one union member at a time. Mirrors
/// PHPStan's `FilterFunctionReturnTypeHelper::determineExactType()`.
///
/// `None` when the call reads its input from elsewhere (`filter_input()`),
/// asks for an array, or any member of the input could go either way, which
/// leaves the result to what the filter alone decides.
fn filter_known_input_return(args: &CallArgs<'_>, filter: i64, flags: i64) -> Option<PhpType> {
    if flags & FILTER_REQUIRE_ARRAY != 0 {
        return None;
    }
    let input = args.resolve(&args.text("$value")?)?.strip_benevolence();
    let mut members = Vec::new();
    flatten_union(&input, &mut members);
    let mut values: Vec<PhpType> = Vec::new();
    let mut fails = false;
    for member in &members {
        match filter_exact_value(member, filter, flags)? {
            Some(value) => {
                if !values.contains(&value) {
                    values.push(value);
                }
            }
            None => fails = true,
        }
    }
    if fails && flags & FILTER_THROW_ON_FAILURE == 0 {
        values.push(filter_failure(flags, None));
    }
    if values.is_empty() {
        return None;
    }
    let result = PhpType::union(values);
    if flags & FILTER_FORCE_ARRAY != 0 {
        return Some(PhpType::generic_array_val(result));
    }
    Some(result)
}

/// `FILTER_FLAG_STRIP_*` and `FILTER_FLAG_ENCODE_*`: the flags under which
/// `FILTER_DEFAULT` rewrites a string rather than passing it through.
const FILTER_DEFAULT_REWRITE_FLAGS: i64 = 4 | 8 | 16 | 32 | 64 | 512;
const FILTER_FLAG_ALLOW_OCTAL: i64 = 1;
const FILTER_FLAG_ALLOW_HEX: i64 = 2;

/// What `filter` makes of a value of type `ty`: `Some(Some(value))` when it
/// passes as `value`, `Some(None)` when it always fails, and `None` when
/// the type does not decide.
fn filter_exact_value(ty: &PhpType, filter: i64, flags: i64) -> Option<Option<PhpType>> {
    let is_bool = ty.is_bool() || ty.is_true() || ty.is_false();
    match filter {
        FILTER_VALIDATE_BOOL => {
            if is_bool {
                return Some(Some(ty.clone()));
            }
            if ty.is_null() {
                return Some(None);
            }
        }
        FILTER_VALIDATE_FLOAT => {
            if ty.is_float_subtype() {
                return Some(Some(ty.clone()));
            }
            if let Some(value) = literal_int(ty) {
                return Some(Some(PhpType::literal_float(format!("{:?}", value as f64))));
            }
            if ty.is_int_subtype() {
                return Some(Some(PhpType::float()));
            }
            if ty.is_true() {
                return Some(Some(PhpType::literal_float("1.0")));
            }
            if ty.is_false() || ty.is_null() {
                return Some(None);
            }
        }
        FILTER_VALIDATE_INT => {
            if ty.is_int_subtype() {
                return Some(Some(ty.clone()));
            }
            if ty.is_true() {
                return Some(Some(PhpType::literal_int("1")));
            }
            if ty.is_false() || ty.is_null() {
                return Some(None);
            }
            let literal = ty.as_literal()?;
            if let Some(value) = literal.parse_f64() {
                let whole = value.fract() == 0.0 && value.abs() < i64::MAX as f64;
                return Some(whole.then(|| PhpType::literal_int((value as i64).to_string())));
            }
            let content = literal.string_content()?;
            if flags & (FILTER_FLAG_ALLOW_OCTAL | FILTER_FLAG_ALLOW_HEX) != 0 {
                return None;
            }
            // The filter trims the same characters `trim()` does by default.
            let content = content.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\0']);
            let digits = content.strip_prefix(['+', '-']).unwrap_or(content);
            let canonical = !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && (digits == "0" || !digits.starts_with('0'));
            return Some(
                canonical
                    .then(|| content.parse::<i64>().ok())
                    .flatten()
                    .map(|value| PhpType::literal_int(value.to_string())),
            );
        }
        FILTER_DEFAULT if flags & FILTER_DEFAULT_REWRITE_FLAGS == 0 && ty.is_string_subtype() => {
            return Some(Some(ty.clone()));
        }
        _ => {}
    }
    None
}

/// What a filter produces for one scalar: the filtered value, or the
/// failure value [`filter_failure`] names.
fn filter_scalar_return(filter: i64, flags: i64, options: Option<&PhpType>) -> Option<PhpType> {
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

    let option = |key: &str| filter_option(options, key);
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
    let failure = filter_failure(flags, options);
    // `bool|false` is just `bool`, and a `string` default is a string.
    if failure.is_subtype_of(&filtered) {
        return Some(filtered);
    }
    Some(PhpType::union(vec![filtered, failure]))
}

/// A required entry of the filter's `options` array.
fn filter_option(options: Option<&PhpType>, key: &str) -> Option<PhpType> {
    options
        .and_then(|o| o.shape_entry(key))
        .filter(|entry| !entry.optional)
        .map(|entry| entry.value_type.clone())
}

/// What a filter returns when the value fails it: the `default` option,
/// else `null` under `FILTER_NULL_ON_FAILURE` and `false` otherwise.
fn filter_failure(flags: i64, options: Option<&PhpType>) -> PhpType {
    filter_option(options, "default").unwrap_or_else(|| {
        if flags & FILTER_NULL_ON_FAILURE != 0 {
            PhpType::null()
        } else {
            PhpType::false_()
        }
    })
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

/// Every string `ty` may be, when it is a literal or a union of them.
fn literal_strings(ty: &PhpType) -> Option<Vec<String>> {
    ty.union_members()
        .into_iter()
        .map(|member| Some(member.as_literal()?.string_content()?.into_owned()))
        .collect()
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
