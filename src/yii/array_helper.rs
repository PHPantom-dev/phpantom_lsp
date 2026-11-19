//! The key paths `yiisoft/arrays`' `ArrayHelper` reads.
//!
//! ```php
//! ArrayHelper::getValue($user, ['profile', 'address']);
//! ArrayHelper::getValueByPath($user, 'profile.address.city');
//! ```
//!
//! `getValue()`, `setValue()`, `keyExists()`, and `remove()` take a key, or
//! a list of keys that walks into the value one key at a time.  Their
//! `ByPath` siblings and `pathExists()` take the same, with each string
//! split on a delimiter (`.` unless told otherwise, `\` escaping it).  A
//! key reads an array's element and, for the two getters, which read
//! through `ArrayHelper::getRootValue()`, an object's property, or calls
//! its method when written `name()`.
//!
//! The walk itself is the shared one in [`crate::type_engine::key_path`];
//! this module says which calls take a path, how their paths split, and
//! what the getters return.  Completion lives in
//! `completion/yii_array_paths.rs`, which also answers go-to-definition.

use crate::php_type::PhpType;
use crate::type_engine::key_path::{self, KeyPathRules};
use crate::type_engine::resolver::ResolutionCtx;

/// The class whose static methods read key paths.
pub(crate) const ARRAY_HELPER: &str = "Yiisoft\\Arrays\\ArrayHelper";

/// A method of `ArrayHelper` that takes a key path into its first
/// argument, `$array`.
#[derive(Debug)]
pub(crate) struct PathMethod {
    name: &'static str,
    /// The name of the parameter that takes the path.
    pub(crate) key_param: &'static str,
    /// Whether a string in the path is split on the delimiter.
    pub(crate) split: bool,
    /// Where the method takes its delimiter, when it takes one.
    delimiter_at: Option<usize>,
    /// Whether a key reads an object's property or method, not only an
    /// array's element.
    objects: bool,
    /// Whether the method returns the value the path reaches, with its
    /// `$default` third.
    returns_value: bool,
}

impl PathMethod {
    /// What a step of this method's path can read.
    pub(crate) fn rules(&self) -> KeyPathRules {
        KeyPathRules {
            properties: self.objects,
            method_calls: self.objects,
        }
    }
}

const PATH_METHODS: &[PathMethod] = &[
    PathMethod {
        name: "getValue",
        key_param: "key",
        split: false,
        delimiter_at: None,
        objects: true,
        returns_value: true,
    },
    PathMethod {
        name: "getValueByPath",
        key_param: "path",
        split: true,
        delimiter_at: Some(3),
        objects: true,
        returns_value: true,
    },
    PathMethod {
        name: "setValue",
        key_param: "key",
        split: false,
        delimiter_at: None,
        objects: false,
        returns_value: false,
    },
    PathMethod {
        name: "setValueByPath",
        key_param: "path",
        split: true,
        delimiter_at: Some(3),
        objects: false,
        returns_value: false,
    },
    PathMethod {
        name: "remove",
        key_param: "key",
        split: false,
        delimiter_at: None,
        objects: false,
        returns_value: true,
    },
    PathMethod {
        name: "removeByPath",
        key_param: "path",
        split: true,
        delimiter_at: Some(3),
        objects: false,
        returns_value: true,
    },
    PathMethod {
        name: "keyExists",
        key_param: "key",
        split: false,
        delimiter_at: None,
        objects: false,
        returns_value: false,
    },
    PathMethod {
        name: "pathExists",
        key_param: "path",
        split: true,
        delimiter_at: Some(3),
        objects: false,
        returns_value: false,
    },
];

/// The method of `ArrayHelper` named `method` that takes a key path, when
/// `class` is `ArrayHelper`.
pub(crate) fn path_method(class: &str, method: &str) -> Option<&'static PathMethod> {
    if !class
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(ARRAY_HELPER)
    {
        return None;
    }
    PATH_METHODS
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(method))
}

/// The argument at `index` of a call whose arguments are `args`, passed in
/// place or by `name`.
pub(crate) fn text_argument<'a>(args: &[&'a str], index: usize, name: &str) -> Option<&'a str> {
    let mut found = None;
    for (at, arg) in args.iter().enumerate() {
        match crate::call_args::split_named_text_arg(arg) {
            (Some(label), value) if label == name => return Some(value),
            (None, value) if at == index && !value.starts_with("...") => found = Some(value),
            _ => {}
        }
    }
    found.filter(|value| !value.is_empty())
}

/// The delimiter `method` splits its path on, read from the call's
/// arguments: `.` unless a literal one-character string says otherwise.
/// `None` when the call passes one that is not a literal.
pub(crate) fn delimiter(method: &PathMethod, args: &[&str]) -> Option<char> {
    let Some(at) = method.delimiter_at else {
        return Some('.');
    };
    let Some(text) = text_argument(args, at, "delimiter") else {
        return Some('.');
    };
    let literal = crate::util::unescape_php_string_literal_cow(text)?;
    let mut chars = literal.chars();
    match (chars.next(), chars.next()) {
        (Some(delimiter), None) if delimiter != '\\' => Some(delimiter),
        _ => None,
    }
}

/// Split `path` into its keys the way `StringHelper::parsePath()` does: on
/// each `delimiter` not escaped by a `\`, with `\\` and an escaped
/// delimiter unescaped.
pub(crate) fn split_path(path: &str, delimiter: char) -> Vec<String> {
    let mut segments = vec![String::new()];
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        let segment = segments.last_mut().expect("seeded with one segment");
        if c == '\\' {
            match chars.peek() {
                Some(&next) if next == '\\' || next == delimiter => {
                    segment.push(next);
                    chars.next();
                }
                _ => segment.push(c),
            }
        } else if c == delimiter {
            segments.push(String::new());
        } else {
            segment.push(c);
        }
    }
    segments
}

/// The keys the path argument `key` names, as `method` reads it.  `None`
/// when any of them is not written as a literal.
pub(crate) fn path_keys(method: &PathMethod, key: &str, delimiter: char) -> Option<Vec<String>> {
    let mut keys = Vec::new();
    push_keys(method, key, delimiter, true, &mut keys)?;
    Some(keys)
}

fn push_keys(
    method: &PathMethod,
    key: &str,
    delimiter: char,
    outermost: bool,
    keys: &mut Vec<String>,
) -> Option<()> {
    let key = key.trim();
    if let Some(elements) = key
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
    {
        // `getValue()` reads a list of keys but no nested list, which its
        // `ByPath` sibling flattens.
        if !outermost && !method.split {
            return None;
        }
        for element in crate::text_scan::split_top_level_commas(elements) {
            if element.contains("=>") {
                return None;
            }
            push_keys(method, element, delimiter, false, keys)?;
        }
        return Some(());
    }
    if key.starts_with('"') && key.contains('$') {
        return None;
    }
    if let Some(literal) = crate::util::unescape_php_string_literal_cow(key) {
        if method.split {
            keys.extend(split_path(&literal, delimiter));
        } else {
            keys.push(literal.into_owned());
        }
        return Some(());
    }
    // An integer key, as written.
    (!key.is_empty() && key.bytes().all(|b| b.is_ascii_digit())).then(|| keys.push(key.to_string()))
}

/// What a call to `method` returns, given the type of the value it reads,
/// the keys its path names, and the type of its default.  `None` when the
/// method returns no value it reads, or the types say nothing about the
/// value the path reaches.
pub(crate) fn path_value_type(
    method: &PathMethod,
    value: &PhpType,
    keys: &[String],
    default: Option<PhpType>,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if !method.returns_value || keys.is_empty() {
        return None;
    }
    let reached = key_path::path_type(value, keys.iter().map(String::as_str), method.rules(), ctx)?;
    if !reached.may_miss {
        return Some(reached.ty);
    }
    Some(PhpType::union(vec![
        reached.ty,
        default.unwrap_or_else(PhpType::null),
    ]))
}

/// What the call `method(args)` on `ArrayHelper` returns, with each
/// argument typed by `arg_type`.
pub(crate) fn call_return_type(
    method: &PathMethod,
    args: &[&str],
    arg_type: &dyn Fn(&str) -> Option<PhpType>,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if !method.returns_value {
        return None;
    }
    let value = arg_type(text_argument(args, 0, "array")?)?;
    let keys = path_keys(
        method,
        text_argument(args, 1, method.key_param)?,
        delimiter(method, args)?,
    )?;
    let default = match text_argument(args, 2, "default") {
        Some(text) => Some(arg_type(text)?),
        None => None,
    };
    path_value_type(method, &value, &keys, default, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_path_reads_escapes_like_parse_path() {
        assert_eq!(split_path("a.b.c", '.'), ["a", "b", "c"]);
        assert_eq!(split_path("a\\.b.c", '.'), ["a.b", "c"]);
        assert_eq!(split_path("a\\\\.b", '.'), ["a\\", "b"]);
        assert_eq!(split_path("a\\x", '.'), ["a\\x"]);
        assert_eq!(split_path("a/b", '/'), ["a", "b"]);
        assert_eq!(split_path("", '.'), [""]);
    }

    #[test]
    fn path_keys_reads_strings_lists_and_integers() {
        let get = path_method(ARRAY_HELPER, "getValue").unwrap();
        let by_path = path_method(ARRAY_HELPER, "getValueByPath").unwrap();
        assert_eq!(path_keys(get, "'a.b'", '.').unwrap(), ["a.b"]);
        assert_eq!(path_keys(get, "['a', 'b']", '.').unwrap(), ["a", "b"]);
        assert_eq!(path_keys(get, "0", '.').unwrap(), ["0"]);
        assert!(path_keys(get, "[['a']]", '.').is_none());
        assert!(path_keys(get, "$key", '.').is_none());
        assert!(path_keys(get, "\"a.$b\"", '.').is_none());
        assert_eq!(path_keys(by_path, "'a.b'", '.').unwrap(), ["a", "b"]);
        assert_eq!(
            path_keys(by_path, "['a.b', ['c'], 1]", '.').unwrap(),
            ["a", "b", "c", "1"]
        );
    }

    #[test]
    fn delimiter_reads_a_literal_argument() {
        let by_path = path_method(ARRAY_HELPER, "getValueByPath").unwrap();
        assert_eq!(delimiter(by_path, &["$a", "'a/b'"]), Some('.'));
        assert_eq!(
            delimiter(by_path, &["$a", "'a/b'", "null", "'/'"]),
            Some('/')
        );
        assert_eq!(
            delimiter(by_path, &["$a", "'a/b'", "delimiter: '/'"]),
            Some('/')
        );
        assert_eq!(delimiter(by_path, &["$a", "'a/b'", "null", "$d"]), None);
    }

    #[test]
    fn path_method_names_only_array_helper() {
        assert!(path_method("\\Yiisoft\\Arrays\\ArrayHelper", "GETVALUE").is_some());
        assert!(path_method("Acme\\ArrayHelper", "getValue").is_none());
        assert!(path_method(ARRAY_HELPER, "merge").is_none());
    }
}
