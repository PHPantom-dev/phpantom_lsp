//! WordPress functions whose arguments decide a return type no PHPDoc type
//! can spell.
//!
//! `src/wordpress/overrides.php` gives the WordPress stubs the conditional
//! return types an argument decides. What is left here needs the arguments
//! themselves, the way PHPStan's WordPress extension reads them:
//!
//! - `wp_parse_args()` merges the defaults under the arguments, and
//!   `shortcode_atts()` keeps the keys of its defaults, so the result is a
//!   shape built from both.
//! - `esc_sql()` and `wp_slash()` walk an array, so the shape they are
//!   handed comes back with each value transformed in place.
//! - The string helpers called on a literal give the literal WordPress
//!   returns, the way `string_func_rules` folds PHP's own.
//!
//! A call these rules cannot pin down falls through to the conditional
//! return type, so every rule declines rather than guess.

use std::borrow::Cow;

use crate::php_type::{PhpType, ShapeEntry, TypeKind, is_array_like_name};
use crate::type_engine::variable::array_func_rules::{ArrayFuncArgs, array_func_raw_type};

use super::CallArgs;

/// The WordPress functions [`return_type`] knows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WordPressFunction {
    ParseArgs,
    ShortcodeAtts,
    EscSql,
    Slash,
    TrailingSlashIt,
    UntrailingSlashIt,
    BackslashIt,
    NormalizeWhitespace,
    StripslashesFromStringsOnly,
    ParseUrl,
}

const FUNCTIONS: &[(&str, WordPressFunction)] = &[
    ("wp_parse_args", WordPressFunction::ParseArgs),
    ("shortcode_atts", WordPressFunction::ShortcodeAtts),
    ("esc_sql", WordPressFunction::EscSql),
    ("wp_slash", WordPressFunction::Slash),
    ("trailingslashit", WordPressFunction::TrailingSlashIt),
    ("untrailingslashit", WordPressFunction::UntrailingSlashIt),
    ("backslashit", WordPressFunction::BackslashIt),
    (
        "normalize_whitespace",
        WordPressFunction::NormalizeWhitespace,
    ),
    (
        "stripslashes_from_strings_only",
        WordPressFunction::StripslashesFromStringsOnly,
    ),
    ("wp_parse_url", WordPressFunction::ParseUrl),
];

pub(super) fn classify(name: &str) -> Option<WordPressFunction> {
    FUNCTIONS
        .iter()
        .find(|(function, _)| name.eq_ignore_ascii_case(function))
        .map(|&(_, function)| function)
}

/// How many alternatives a folded union may carry, as in
/// `string_func_rules`.
const MAX_ALTERNATIVES: usize = 32;

/// The return type this call's arguments decide, or `None` to leave the
/// call to the declared return type.
pub(super) fn return_type(function: WordPressFunction, args: &CallArgs<'_>) -> Option<PhpType> {
    let arg = |param: &str| args.resolve(&args.text(param)?);
    match function {
        WordPressFunction::ParseArgs => parse_args(args),
        WordPressFunction::ShortcodeAtts => shortcode_atts(&arg("$pairs")?, &arg("$atts")?),
        WordPressFunction::EscSql => escape_sql(&arg("$data")?),
        WordPressFunction::Slash => {
            let value = arg("$value")?;
            (!value.is_mixed()).then(|| slash(&value))
        }
        WordPressFunction::TrailingSlashIt => fold_literals(&arg("$value")?, |s| {
            format!("{}/", s.trim_end_matches(['/', '\\']))
        }),
        WordPressFunction::UntrailingSlashIt => fold_literals(&arg("$value")?, |s| {
            s.trim_end_matches(['/', '\\']).to_string()
        }),
        WordPressFunction::BackslashIt => fold_literals(&arg("$value")?, backslashit),
        WordPressFunction::NormalizeWhitespace => {
            fold_literals(&arg("$str")?, normalize_whitespace)
        }
        WordPressFunction::StripslashesFromStringsOnly => {
            stripslashes_from_strings_only(&arg("$value")?)
        }
        WordPressFunction::ParseUrl => parse_url(args),
    }
}

// ─── Merged shapes ──────────────────────────────────────────────────────────

/// `wp_parse_args($args, $defaults)`: the arguments as an array, merged over
/// the defaults with `array_merge()` when they are a non-empty array.
///
/// An object argument is read through `get_object_vars()` from inside
/// WordPress, which sees fewer properties than the caller can, and a query
/// string is parsed at runtime, so only an array argument is described.
fn parse_args(args: &CallArgs<'_>) -> Option<PhpType> {
    let parsed = args.resolve(&args.text("$args")?)?;
    if !alternatives(&parsed).iter().all(is_array) {
        return None;
    }
    let Some(defaults) = args.text("$defaults") else {
        return Some(parsed);
    };
    let defaults = args.resolve(&defaults)?;
    let members = alternatives(&defaults);
    // Defaults that are not an array, or an empty one, are skipped.
    if members.iter().all(|m| !m.is_array_like() && !m.is_mixed())
        || defaults.is_empty_array_shape()
    {
        return Some(parsed);
    }
    if !members.iter().all(is_array) {
        return None;
    }
    array_func_raw_type("array_merge", &TypeArgs(&[defaults, parsed]))
}

/// `shortcode_atts($pairs, $atts)`: each key of the defaults, holding the
/// attribute given for it or the default.
///
/// The attributes of a parsed shortcode are strings, so attributes that say
/// nothing about their values (an untyped shortcode callback's `$atts`, or
/// the `''` WordPress passes when there are none) are read as strings, as
/// PHPStan's WordPress extension does.
fn shortcode_atts(pairs: &PhpType, atts: &PhpType) -> Option<PhpType> {
    let attribute = if is_array(atts) {
        atts.iterable_element_type()
            .filter(|value| !value.is_mixed())
            .unwrap_or_else(PhpType::string)
    } else if alternatives(atts)
        .iter()
        .all(|m| is_array(m) || m.is_mixed() || m.is_string_subtype() || m.is_null())
    {
        PhpType::string()
    } else {
        return None;
    };
    let Some(entries) = sealed_shape_entries(pairs) else {
        // Defaults of an unknown shape keep their keys, each value the
        // default or what was given.
        if !is_array(pairs) {
            return None;
        }
        if pairs.is_bare_array() || atts.is_empty_array_shape() {
            return Some(pairs.clone());
        }
        let TypeKind::Generic(generic) = pairs.kind() else {
            return None;
        };
        let mut type_args = generic.args.clone();
        let value = type_args.last_mut()?;
        *value = PhpType::join_runtime_value_types(vec![
            value.clone(),
            attribute.widen_scalar_literals(),
        ]);
        return Some(PhpType::generic_atom(generic.name, type_args));
    };
    let atts_entries = is_array(atts).then(|| sealed_shape_entries(atts)).flatten();
    let mut out: Vec<ShapeEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        let key = entry.key.as_deref()?;
        let value = match &atts_entries {
            Some(atts_entries) => {
                match atts_entries.iter().find(|a| a.key.as_deref() == Some(key)) {
                    Some(given) if !given.optional => given.value_type.clone(),
                    Some(given) => PhpType::join_runtime_value_types(vec![
                        entry.value_type.clone(),
                        given.value_type.clone(),
                    ]),
                    None => entry.value_type.clone(),
                }
            }
            None => {
                PhpType::join_runtime_value_types(vec![entry.value_type.clone(), attribute.clone()])
            }
        };
        out.push(ShapeEntry {
            key: entry.key.clone(),
            value_type: value,
            optional: entry.optional,
        });
    }
    Some(PhpType::array_shape(out))
}

/// The entries of a sealed array shape, each with its key spelled out, or
/// `None` for anything else.
fn sealed_shape_entries(ty: &PhpType) -> Option<Vec<ShapeEntry>> {
    let parts = ty.shape_parts()?;
    if parts.tail.is_some() {
        return None;
    }
    // A positional entry's key is its position among the positional ones,
    // which only holds while no explicit integer key comes before it.
    let mut next_index = 0;
    parts
        .entries
        .iter()
        .map(|entry| {
            let key = match &entry.key {
                Some(key) if key.parse::<i64>().is_ok() => return None,
                Some(key) => key.clone(),
                None => {
                    next_index += 1;
                    (next_index - 1).to_string()
                }
            };
            Some(ShapeEntry {
                key: Some(key),
                ..entry.clone()
            })
        })
        .collect()
}

/// Two array types read as the arguments of an `array_merge()` call.
struct TypeArgs<'a>(&'a [PhpType]);

impl ArrayFuncArgs for TypeArgs<'_> {
    fn arg_raw_type(&self, index: usize) -> Option<PhpType> {
        self.0.get(index).cloned()
    }

    fn bool_literal(&self, _index: usize) -> Option<bool> {
        None
    }

    fn has_arg(&self, index: usize) -> bool {
        index < self.0.len()
    }

    fn is_spread(&self, _index: usize) -> bool {
        false
    }

    fn is_named(&self, _index: usize) -> bool {
        false
    }

    fn callback_declared_return_type(&self, _index: usize) -> Option<PhpType> {
        None
    }

    fn callback_inferred_return_type(
        &self,
        _index: usize,
        _param_type: &PhpType,
    ) -> Option<PhpType> {
        None
    }

    fn narrows(&self, _inferred: &PhpType, _declared: &PhpType) -> bool {
        false
    }

    fn arg_atom_text(&self, _index: usize) -> Option<String> {
        None
    }

    fn callback_param_narrowing(
        &self,
        _index: usize,
        _param_index: usize,
        _subject: &PhpType,
    ) -> Option<PhpType> {
        None
    }
}

// ─── Arrays mapped in place ─────────────────────────────────────────────────

/// `esc_sql($data)`: an array keeps its shape, nested arrays included, each
/// scalar escaped to a `string` and anything else replaced by `''`.
fn escape_sql(ty: &PhpType) -> Option<PhpType> {
    let members = alternatives(ty)
        .iter()
        .map(|member| {
            if member.is_mixed() {
                None
            } else if is_array(member) {
                Some(map_array_values(member, &|value| {
                    escape_sql(value).unwrap_or_else(|| value.clone())
                }))
            } else if member.is_subtype_of(&scalar()) {
                Some(PhpType::string())
            } else if member.is_null() || member.is_object_like() || member.is_resource() {
                Some(PhpType::literal_string_value(""))
            } else {
                None
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(union_of(members))
}

/// `wp_slash($value)`: a string with its quotes and backslashes escaped,
/// an array with each of its values slashed, anything else unchanged.
fn slash(ty: &PhpType) -> PhpType {
    let members = alternatives(ty)
        .iter()
        .map(|member| {
            if let Some(value) = literal_string(member) {
                literal(&addslashes(&value)).unwrap_or_else(PhpType::string)
            } else if member.is_string_subtype() {
                if keeps_refinements_when_slashed(member) {
                    member.clone()
                } else {
                    PhpType::string()
                }
            } else if is_array(member) {
                map_array_values(member, &slash)
            } else {
                member.clone()
            }
        })
        .collect();
    union_of(members)
}

/// String refinements that adding backslashes keeps: it lengthens a string
/// and never touches a letter or a digit. A class name is not among them,
/// since a slashed `Foo\Bar` no longer names the class.
const KEPT_BY_SLASHING: &[&str] = &[
    "string",
    "non-empty-string",
    "non-falsy-string",
    "truthy-string",
    "lowercase-string",
    "uppercase-string",
    "non-empty-lowercase-string",
    "non-empty-uppercase-string",
    "numeric-string",
    "literal-string",
    "non-empty-literal-string",
];

fn keeps_refinements_when_slashed(ty: &PhpType) -> bool {
    ty.intersection_members().iter().all(|part| {
        matches!(part.kind(), TypeKind::Named(name)
            if KEPT_BY_SLASHING.iter().any(|kept| name.eq_ignore_ascii_case(kept)))
    })
}

/// `ty`, an array type, with `map` applied to the type of each of its
/// values and every key left where it was.
fn map_array_values(ty: &PhpType, map: &dyn Fn(&PhpType) -> PhpType) -> PhpType {
    if let Some(parts) = ty.shape_parts() {
        let entries: Vec<ShapeEntry> = parts
            .entries
            .iter()
            .map(|entry| ShapeEntry {
                value_type: map(&entry.value_type),
                ..entry.clone()
            })
            .collect();
        let shape = match parts.is_list {
            true => PhpType::list_shape(entries),
            false => PhpType::array_shape(entries),
        };
        return match parts.tail {
            Some((key, value)) => PhpType::unsealed_shape(shape, key.clone(), map(value)),
            None => shape,
        };
    }
    match ty.kind() {
        TypeKind::Generic(generic) => {
            let mut args = generic.args.clone();
            if let Some(value) = args.last_mut() {
                *value = map(value);
            }
            PhpType::generic_atom(generic.name, args)
        }
        TypeKind::Array(value) => PhpType::array_of(map(value)),
        _ => ty.clone(),
    }
}

// ─── Calls on literal arguments ─────────────────────────────────────────────

/// `fold` applied to each literal string `ty` may be, or `None` unless every
/// alternative is a literal string.
fn fold_literals(ty: &PhpType, fold: impl Fn(&str) -> String) -> Option<PhpType> {
    let members = alternatives(ty);
    if members.len() > MAX_ALTERNATIVES {
        return None;
    }
    let folded = members
        .iter()
        .map(|member| literal(&fold(&literal_string(member)?)))
        .collect::<Option<Vec<_>>>()?;
    Some(union_of(folded))
}

/// `stripslashes_from_strings_only($value)`: a literal string unslashed,
/// anything that is not a string unchanged.
fn stripslashes_from_strings_only(ty: &PhpType) -> Option<PhpType> {
    let members = alternatives(ty);
    if members.len() > MAX_ALTERNATIVES {
        return None;
    }
    let mut folded_any = false;
    let mut out = Vec::with_capacity(members.len());
    for member in &members {
        if let Some(value) = literal_string(member) {
            out.push(literal(&stripslashes(&value))?);
            folded_any = true;
        } else if is_array(member)
            || member.is_null()
            || member.is_bool()
            || member.is_int_subtype()
            || member.is_float_subtype()
            || (member.is_object_like() && !member.is_string_subtype())
        {
            out.push(member.clone());
        } else {
            return None;
        }
    }
    folded_any.then(|| union_of(out))
}

/// PHP's `addslashes()`.
fn addslashes(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\'' | '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '\0' => out.push_str("\\0"),
            _ => out.push(ch),
        }
    }
    out
}

/// PHP's `stripslashes()`.
fn stripslashes(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('0') => out.push('\0'),
            Some(escaped) => out.push(escaped),
            None => {}
        }
    }
    out
}

/// WordPress's `backslashit()`: a leading digit gains two backslashes, and
/// every ASCII letter one.
fn backslashit(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2 + 2);
    if value.starts_with(|c: char| c.is_ascii_digit()) {
        out.push_str("\\\\");
    }
    for ch in value.chars() {
        if ch.is_ascii_alphabetic() {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// WordPress's `normalize_whitespace()`: trimmed, carriage returns turned
/// into newlines, and each run of newlines, or of spaces and tabs, made one.
fn normalize_whitespace(value: &str) -> String {
    let trimmed = value.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0B']);
    let mut out = String::with_capacity(trimmed.len());
    let mut last: Option<char> = None;
    for ch in trimmed.chars() {
        let ch = match ch {
            '\r' | '\n' => '\n',
            ' ' | '\t' => ' ',
            _ => ch,
        };
        if matches!(ch, '\n' | ' ') && last == Some(ch) {
            continue;
        }
        out.push(ch);
        last = Some(ch);
    }
    out
}

// ─── wp_parse_url() ─────────────────────────────────────────────────────────

/// The `PHP_URL_*` constants, by value, as `_wp_translate_php_url_constant_to_key()`
/// names the key each one reads.
const URL_COMPONENTS: [&str; 8] = [
    "scheme", "host", "port", "user", "pass", "path", "query", "fragment",
];

/// `wp_parse_url($url, $component)` on a literal URL: the parts
/// `parse_url()` finds, or the one the component asks for (`null` when the
/// URL has no such part, or no `PHP_URL_*` constant has that value).
fn parse_url(args: &CallArgs<'_>) -> Option<PhpType> {
    let url = args.resolve(&args.text("$url")?)?;
    let component = match args.text("$component") {
        Some(text) => super::literal_int(&args.resolve(&text)?)?,
        None => -1,
    };
    let members = alternatives(&url);
    if members.len() > MAX_ALTERNATIVES {
        return None;
    }
    let results = members
        .iter()
        .map(|member| {
            let parts = url_parts(&literal_string(member)?)?;
            Some(match component {
                -1 => PhpType::array_shape(
                    parts
                        .into_iter()
                        .map(|(key, value)| ShapeEntry {
                            key: Some(key.to_string()),
                            value_type: value,
                            optional: false,
                        })
                        .collect(),
                ),
                _ => usize::try_from(component)
                    .ok()
                    .and_then(|index| URL_COMPONENTS.get(index))
                    .and_then(|key| parts.into_iter().find(|(k, _)| k == key))
                    .map_or_else(PhpType::null, |(_, value)| value),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(union_of(results))
}

/// The parts `parse_url()` gives a URL written `scheme://host[:port]` with
/// an optional path, query and fragment, in the order it gives them.
///
/// `None` for every other spelling (credentials, an IPv6 host, a relative
/// or scheme-relative URL, whitespace or control characters, an empty
/// part), whose parsing has edge cases not worth reproducing.
fn url_parts(url: &str) -> Option<Vec<(&'static str, PhpType)>> {
    if url
        .bytes()
        .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    let mut scheme_chars = scheme.chars();
    if !scheme_chars.next()?.is_ascii_alphabetic()
        || !scheme_chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let (rest, fragment) = split_part(rest, '#')?;
    let (rest, query) = split_part(rest, '?')?;
    let (authority, path) = match rest.find('/') {
        Some(slash) => rest.split_at(slash),
        None => (rest, ""),
    };
    if authority.contains(['@', '[', ']']) {
        return None;
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let port: u32 = port.parse().ok()?;
            if port > 65535 {
                return None;
            }
            (host, Some(port))
        }
        None => (authority, None),
    };
    if host.is_empty() {
        return None;
    }

    let mut parts = vec![
        ("scheme", PhpType::literal_string_value(scheme)),
        ("host", PhpType::literal_string_value(host)),
    ];
    if let Some(port) = port {
        parts.push(("port", PhpType::literal_int(port.to_string())));
    }
    for (key, value) in [
        ("path", Some(path)),
        ("query", query),
        ("fragment", fragment),
    ] {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            parts.push((key, PhpType::literal_string_value(value)));
        }
    }
    Some(parts)
}

/// `text` split at the first `separator`, or kept whole when it has none.
/// `None` when the part after the separator is empty.
fn split_part(text: &str, separator: char) -> Option<(&str, Option<&str>)> {
    match text.split_once(separator) {
        Some((_, "")) => None,
        Some((before, after)) => Some((before, Some(after))),
        None => Some((text, None)),
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// The alternatives of `ty`, a nullable type's `null` among them.
fn alternatives(ty: &PhpType) -> Vec<PhpType> {
    let mut out = Vec::new();
    for member in ty.union_members() {
        match member.kind() {
            TypeKind::Nullable(inner) => {
                out.push(inner.clone());
                out.push(PhpType::null());
            }
            _ => out.push(member.clone()),
        }
    }
    out
}

/// Whether `ty` is an array, rather than an `iterable` that may be an
/// object.
fn is_array(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => is_array_like_name(name) && !name.eq_ignore_ascii_case("iterable"),
        TypeKind::Generic(generic) => {
            is_array_like_name(&generic.name) && !generic.name.eq_ignore_ascii_case("iterable")
        }
        TypeKind::Array(_) | TypeKind::ArrayShape(_) => true,
        _ => false,
    }
}

fn scalar() -> PhpType {
    PhpType::union(vec![
        PhpType::int(),
        PhpType::float(),
        PhpType::string(),
        PhpType::bool(),
    ])
}

fn literal_string(ty: &PhpType) -> Option<String> {
    ty.as_literal()?.string_content().map(Cow::into_owned)
}

/// The literal string type of `value`, or `None` when it holds a control
/// character the type's spelling cannot carry.
fn literal(value: &str) -> Option<PhpType> {
    (!value.bytes().any(|b| b.is_ascii_control())).then(|| PhpType::literal_string_value(value))
}

fn union_of(members: Vec<PhpType>) -> PhpType {
    PhpType::join_runtime_value_types(members)
}

#[cfg(test)]
#[path = "wordpress_tests.rs"]
mod tests;
