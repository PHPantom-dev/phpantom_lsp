//! Qualifying the class names in a class constant's initializer.
//!
//! An initializer is kept as source text (`ConstantInfo::value`) and read
//! by whichever file reads the constant, where the imports and namespace of
//! the block that wrote it are not in scope.  The classes it names are
//! therefore spelled out fully qualified while the declaring file is parsed,
//! the way every other name a class carries is.

use crate::text_scan;

/// `expression` with the class of every `Class::member` access in it
/// (`Defaults::MODE`, `Defaults::class`, `Suit::Hearts->value`) spelled
/// fully qualified, with a leading `\`, or `None` when no class in it needs
/// that.
///
/// `resolve` qualifies a relative name against the imports and namespace of
/// the block `expression` was written in, and `namespace` is that block's
/// namespace, which a `namespace\Foo` name is relative to.  A name already
/// written fully qualified is kept, and so are `self`, `static` and
/// `parent`, which name a class only the reader of the constant knows.
/// Names inside strings and comments are not class names, and neither is a
/// bare name (a global constant) or the member after `::` or `->`.
pub(crate) fn qualify_class_names(
    expression: &str,
    namespace: Option<&str>,
    resolve: &dyn Fn(&str) -> String,
) -> Option<String> {
    // Nothing is read off a class without `::`, which keeps the scan off
    // the plain literals most initializers are.
    if !expression.contains("::") {
        return None;
    }
    let bytes = expression.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    // Whether the last token was `::` or `->`, after which a name is the
    // member being read rather than a class.
    let mut after_access = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' || b == b'"' {
            i = text_scan::skip_string_forward(bytes, i);
            after_access = false;
        } else if let Some(end) = text_scan::skip_php_comment(bytes, i) {
            i = end;
        } else if bytes[i..].starts_with(b"<<<") {
            i = skip_heredoc(bytes, i);
            after_access = false;
        } else if bytes[i..].starts_with(b"::") || bytes[i..].starts_with(b"->") {
            i += 2;
            after_access = true;
        } else if b == b'$' || b.is_ascii_digit() {
            // The letters of a variable or of a number (`0xFF`, `1e3`) are
            // not a name.
            i += 1;
            while bytes.get(i).is_some_and(|&b| is_name_byte(b)) {
                i += 1;
            }
            after_access = false;
        } else if is_name_start(b) || (b == b'\\' && starts_name(bytes, i + 1)) {
            let start = i;
            i += 1;
            while bytes
                .get(i)
                .is_some_and(|&b| is_name_byte(b) || (b == b'\\' && starts_name(bytes, i + 1)))
            {
                i += 1;
            }
            if std::mem::take(&mut after_access) || !followed_by_access(bytes, i) {
                continue;
            }
            if let Some(fqn) = qualify(&expression[start..i], namespace, resolve) {
                out.push_str(&expression[copied..start]);
                out.push('\\');
                out.push_str(&fqn);
                copied = i;
            }
        } else {
            if !b.is_ascii_whitespace() {
                after_access = false;
            }
            i += 1;
        }
    }
    if copied == 0 {
        return None;
    }
    out.push_str(&expression[copied..]);
    Some(out)
}

/// The fully qualified name, without a leading `\`, of the class written as
/// `name` in a block whose namespace is `namespace`, or `None` when `name`
/// is kept as written.
fn qualify(
    name: &str,
    namespace: Option<&str>,
    resolve: &dyn Fn(&str) -> String,
) -> Option<String> {
    if name.starts_with('\\') || crate::class_lookup::is_class_keyword(name) {
        return None;
    }
    // `namespace\Foo` is `Foo` in the current namespace, never an import.
    const NAMESPACE_PREFIX: &str = "namespace\\";
    if name
        .get(..NAMESPACE_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(NAMESPACE_PREFIX))
    {
        let rest = &name[NAMESPACE_PREFIX.len()..];
        return Some(match namespace {
            Some(ns) => format!("{ns}\\{rest}"),
            None => rest.to_string(),
        });
    }
    Some(resolve(name))
}

/// Whether the token after the name ending at `end` is `::`, so that the
/// name is the class a member is read off.
fn followed_by_access(bytes: &[u8], end: usize) -> bool {
    let mut i = end;
    loop {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        match text_scan::skip_php_comment(bytes, i) {
            Some(after) => i = after,
            None => return bytes[i..].starts_with(b"::"),
        }
    }
}

/// The offset just past the heredoc or nowdoc whose `<<<` is at `start`:
/// past its closing label, or the end of `bytes` when it is never closed.
fn skip_heredoc(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 3;
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    if matches!(bytes.get(i), Some(b'\'' | b'"')) {
        i += 1;
    }
    let label_start = i;
    while bytes.get(i).is_some_and(|&b| is_name_byte(b)) {
        i += 1;
    }
    let label = &bytes[label_start..i];
    if label.is_empty() {
        return i;
    }
    // The body starts on the next line, and ends at the first line that,
    // after any indentation, starts with the label and does not continue
    // it.
    while let Some(newline) = text_scan::find_byte(bytes, i, b'\n') {
        i = newline + 1;
        while matches!(bytes.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        let end = i + label.len();
        if bytes[i..].starts_with(label) && !bytes.get(end).is_some_and(|&b| is_name_byte(b)) {
            return end;
        }
    }
    bytes.len()
}

/// Whether a name starts at `at`.
fn starts_name(bytes: &[u8], at: usize) -> bool {
    bytes.get(at).is_some_and(|&b| is_name_start(b))
}

/// Whether `b` can start a PHP name.  Every byte of a multi-byte UTF-8
/// character is one, as PHP allows any of them in a name.
fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

/// Whether `b` can continue a PHP name.
fn is_name_byte(b: u8) -> bool {
    is_name_start(b) || b.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::qualify_class_names;

    /// Resolves the way the declaring file of these tests would: `Defaults`
    /// is imported from `Vendor`, and anything else is in `Lib`.
    fn qualify(expression: &str) -> Option<String> {
        qualify_class_names(expression, Some("Lib"), &|name| match name {
            "Defaults" => "Vendor\\Defaults".to_string(),
            _ => format!("Lib\\{name}"),
        })
    }

    #[test]
    fn qualifies_the_class_of_a_member_access() {
        assert_eq!(
            qualify("Defaults::MODE").as_deref(),
            Some("\\Vendor\\Defaults::MODE")
        );
        assert_eq!(
            qualify("Labels::DEFAULT").as_deref(),
            Some("\\Lib\\Labels::DEFAULT")
        );
        assert_eq!(
            qualify("Defaults::class").as_deref(),
            Some("\\Vendor\\Defaults::class")
        );
        assert_eq!(
            qualify("Sub\\Labels::DEFAULT").as_deref(),
            Some("\\Lib\\Sub\\Labels::DEFAULT")
        );
    }

    #[test]
    fn qualifies_every_access_in_a_compound_expression() {
        assert_eq!(
            qualify("[Defaults::MODE => Labels::A, 'b' => Labels :: B]").as_deref(),
            Some("[\\Vendor\\Defaults::MODE => \\Lib\\Labels::A, 'b' => \\Lib\\Labels :: B]")
        );
        assert_eq!(
            qualify("Defaults::A | Labels::B").as_deref(),
            Some("\\Vendor\\Defaults::A | \\Lib\\Labels::B")
        );
        assert_eq!(
            qualify("Suit::Hearts->value").as_deref(),
            Some("\\Lib\\Suit::Hearts->value")
        );
    }

    #[test]
    fn keeps_names_that_are_not_a_class_to_qualify() {
        // Already fully qualified, or bound by whoever reads the constant.
        assert_eq!(qualify("\\Vendor\\Defaults::MODE"), None);
        assert_eq!(qualify("[self::A, static::B, parent::C, SELF::D]"), None);
        // A member, after `::` or `->`, however it is spaced.
        assert_eq!(qualify("self:: A"), None);
        assert_eq!(qualify("self::A->value"), None);
        // A global constant, a number, and plain literals.
        assert_eq!(qualify("JSON_THROW_ON_ERROR | self::FLAGS"), None);
        assert_eq!(qualify("0x1F | self::E1"), None);
        assert_eq!(qualify("'plain'"), None);
    }

    #[test]
    fn skips_strings_comments_and_heredocs() {
        assert_eq!(qualify("'see Defaults::MODE'"), None);
        assert_eq!(qualify("\"see \\\" Defaults::MODE\""), None);
        assert_eq!(qualify("[\n    // see Defaults::MODE\n    1,\n]"), None);
        assert_eq!(qualify("[/* Defaults::MODE */ 1]"), None);
        assert_eq!(
            qualify("[<<<EOT\n    Defaults::MODE\n    EOT, Labels::A]").as_deref(),
            Some("[<<<EOT\n    Defaults::MODE\n    EOT, \\Lib\\Labels::A]")
        );
        assert_eq!(
            qualify("[<<<'EOT'\nDefaults::MODE\nEOT\n]"),
            None,
            "a nowdoc's body is not code"
        );
    }

    #[test]
    fn a_comment_between_the_class_and_its_member_keeps_it_a_class() {
        assert_eq!(
            qualify("Defaults /* the defaults */ ::MODE").as_deref(),
            Some("\\Vendor\\Defaults /* the defaults */ ::MODE")
        );
    }

    #[test]
    fn a_namespace_relative_name_is_in_the_current_namespace() {
        assert_eq!(
            qualify("namespace\\Defaults::MODE").as_deref(),
            Some("\\Lib\\Defaults::MODE")
        );
        assert_eq!(
            qualify_class_names("namespace\\Defaults::MODE", None, &|name| name.to_string())
                .as_deref(),
            Some("\\Defaults::MODE")
        );
    }

    #[test]
    fn a_global_name_is_still_spelled_fully_qualified() {
        // The reading file's own imports must not get a say in it.
        assert_eq!(
            qualify_class_names("Defaults::MODE", None, &|name| name.to_string()).as_deref(),
            Some("\\Defaults::MODE")
        );
    }
}
