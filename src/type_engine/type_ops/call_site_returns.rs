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
        CallSiteFunction::WordPress(function) => wordpress::return_type(function, &args),
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
    let fqn = class.fqn();
    (class.is_final && !fqn.contains('@')).then(|| vec![fqn.to_string()])
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
