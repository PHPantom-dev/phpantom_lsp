//! Types read from the source text of a literal: a constant's
//! initializer, or an array literal written as a call argument.

use crate::php_type::{PhpType, ShapeEntry, TypeKind};
use crate::text_scan::split_top_level_arrow;

/// Resolver from an expression's source text to its type, as
/// [`crate::Backend::resolve_arg_text_to_type`] provides.
pub(crate) type TextResolver<'a> = &'a dyn Fn(&str) -> Option<PhpType>;

/// The literal an array element that names something resolves to: `'Foo'`
/// for `Foo::class`, the value a constant or an enum case's `->value`
/// holds, or the enum an enum case belongs to.
///
/// A shape entry has to be exact to mean anything (any other element falls
/// back to plain `array`, see [`literal_array_shape`]), so a resolved
/// `class-string<Foo>` is unwrapped down to the class name it names,
/// matching how PHPStan reads a `::class`
/// array element: as the literal string, not the wrapper type a bare
/// `Foo::class` expression resolves to on its own.
fn resolved_element_literal(value: &str, resolve: TextResolver<'_>) -> Option<PhpType> {
    let ty = resolve(value)?;
    match ty.kind() {
        TypeKind::ClassString(Some(inner)) => match inner.kind() {
            TypeKind::Named(name) => {
                Some(PhpType::literal_string_value(name.trim_start_matches('\\')))
            }
            _ => None,
        },
        TypeKind::Literal(_) => Some(ty),
        // An object a constant expression names (an enum case, or a `new`
        // in a global constant) is an instance of exactly that class.
        TypeKind::Named(_) if !ty.is_keyword() => Some(ty),
        _ => None,
    }
}

/// The class name a `Foo::class` expression names, resolved against the
/// reading file.
fn class_const_fetch_name(value: &str, resolve: TextResolver<'_>) -> Option<String> {
    match resolve(value)?.kind() {
        TypeKind::ClassString(Some(inner)) => match inner.kind() {
            TypeKind::Named(name) => Some(name.trim_start_matches('\\').to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// The contents of an array literal's brackets, or `None` when the text
/// is not one.
fn strip_array_literal(value: &str) -> Option<&str> {
    if let Some(rest) = value.strip_prefix('[') {
        return rest.strip_suffix(']');
    }
    let rest = value
        .strip_prefix("array(")
        .or_else(|| value.strip_prefix("array ("))?;
    rest.strip_suffix(')')
}

/// Which elements of an array literal [`literal_array_shape`] may type.
#[derive(Clone, Copy)]
enum ArrayElements<'a> {
    /// A constant's initialiser: literals, plus the names the resolver pins
    /// down to one exact value (see [`resolved_element_literal`]).
    Exact(Option<TextResolver<'a>>),
    /// A call argument, written inline: any element the resolver can type.
    Any(TextResolver<'a>),
}

impl<'a> ArrayElements<'a> {
    fn resolver(self) -> Option<TextResolver<'a>> {
        match self {
            ArrayElements::Exact(resolve) => resolve,
            ArrayElements::Any(resolve) => Some(resolve),
        }
    }

    fn value_type(self, value: &str) -> Option<PhpType> {
        match self {
            // A constant expression cannot interpolate, but an argument can,
            // and an interpolated string is not the literal it is spelled as.
            ArrayElements::Any(resolve) if value.starts_with('"') && value.contains('$') => {
                resolve(value)
            }
            ArrayElements::Any(resolve) => {
                infer_type_from_constant_value_inner(value, self).or_else(|| resolve(value))
            }
            ArrayElements::Exact(resolve) => infer_type_from_constant_value_inner(value, self)
                .or_else(|| resolved_element_literal(value, resolve?)),
        }
    }
}

/// The shape an array literal describes, when every element of it can be
/// typed (see [`ArrayElements`]).
///
/// Returns `None` as soon as an element cannot, because a shape missing
/// one of its slots would claim the array is smaller than it is. The
/// caller then falls back to an unconstrained `array`.
fn literal_array_shape(inner: &str, elements: ArrayElements<'_>) -> Option<PhpType> {
    use crate::php_type::ShapeEntry;

    let resolve = elements.resolver();
    if inner.trim().is_empty() {
        return Some(PhpType::list_shape(Vec::new()));
    }
    let items = crate::text_scan::split_top_level_commas(inner);
    let mut entries: Vec<ShapeEntry> = Vec::new();
    let mut keyed = false;
    for item in items {
        let item = item.trim();
        // A trailing comma leaves an empty final item.
        if item.is_empty() {
            continue;
        }
        // A spread copies a shape's entries across, keeping string keys
        // and renumbering integer ones onto the end.
        if let Some(source) = item.strip_prefix("...") {
            let source = resolve?(source.trim())?;
            for (key, value_type) in
                crate::type_engine::variable::raw_type_inference::spread_entries(&source)?
            {
                keyed |= key.is_some();
                put_shape_entry(&mut entries, key, value_type);
            }
            continue;
        }
        let (key, value) = match split_top_level_arrow(item) {
            Some((key_text, value_text)) => {
                keyed = true;
                let key_text = key_text.trim();
                let key = literal_shape_key(key_text).or_else(|| {
                    // A class name is never a decimal integer, so a
                    // `Foo::class` key stays the string it names.
                    class_const_fetch_name(key_text, resolve?)
                })?;
                (Some(key), value_text)
            }
            None => (None, item),
        };
        let value_type = elements.value_type(value.trim())?;
        put_shape_entry(&mut entries, key, value_type);
    }
    Some(if keyed {
        PhpType::array_shape(entries)
    } else {
        PhpType::list_shape(entries)
    })
}

/// Write `value_type` under `key`, or under the next position when `key` is
/// `None`. A key written again keeps its place and takes the later value,
/// as PHP does.
fn put_shape_entry(entries: &mut Vec<ShapeEntry>, key: Option<String>, value_type: PhpType) {
    if let Some(existing) = key
        .as_ref()
        .and_then(|key| entries.iter_mut().find(|e| e.key.as_ref() == Some(key)))
    {
        existing.value_type = value_type;
        return;
    }
    entries.push(ShapeEntry {
        key,
        value_type,
        optional: false,
    });
}

/// The key text of a shape entry, for the literal keys PHP allows: a
/// quoted string or an integer.
fn literal_shape_key(text: &str) -> Option<String> {
    if let Some(unquoted) = crate::text_scan::unquote_php_string(text) {
        return Some(unquoted.to_string());
    }
    crate::php_type::parse_php_int_literal(text).map(|value| value.to_string())
}

/// Infer a scalar type from a constant's initializer value string.
///
/// Recognises integer literals (`42`, `-1`, `0xFF`), float literals
/// (`3.14`, `1e10`), string literals (`'hello'`, `"world"`), boolean
/// keywords (`true`, `false`), `null`, and array literals (`[...]`,
/// `array(...)`).  Scalar literals keep their value (`1`, `'foo'`)
/// rather than widening to the base type, matching how a literal
/// assignment resolves.  Returns `None` for expressions that cannot be
/// trivially classified (e.g. concatenation, function calls).
pub(crate) fn infer_type_from_constant_value(value: &str) -> Option<PhpType> {
    infer_type_from_constant_value_inner(value, ArrayElements::Exact(None))
}

/// [`infer_type_from_constant_value`], but an array element that the fixed
/// literal patterns below can't classify (currently just `Foo::class`) is
/// offered to `resolve` before the whole array gives up and falls back to
/// plain `array`. Text scanning alone can tell that `Foo::class` names a
/// class, but not which one: `Foo` may need qualifying against the
/// reading file's namespace or `use` imports, which only a resolver with
/// access to the class loader can do.
pub(crate) fn infer_type_from_constant_value_resolved(
    value: &str,
    resolve: TextResolver<'_>,
) -> Option<PhpType> {
    infer_type_from_constant_value_inner(value, ArrayElements::Exact(Some(resolve)))
}

/// The type of an array literal written as a call argument (`f(['a' =>
/// $b])`), with each element typed through `resolve`, or `None` when `text`
/// is not an array literal.
///
/// An element `resolve` cannot type leaves the whole literal a plain
/// `array`.
pub(crate) fn infer_array_literal_type(text: &str, resolve: TextResolver<'_>) -> Option<PhpType> {
    let inner = strip_array_literal(text.trim())?;
    Some(literal_array_shape(inner, ArrayElements::Any(resolve)).unwrap_or_else(PhpType::array))
}

fn infer_type_from_constant_value_inner(
    value: &str,
    elements: ArrayElements<'_>,
) -> Option<PhpType> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }

    // String literals: single or double quoted.
    if v.len() >= 2
        && ((v.starts_with('\'') && v.ends_with('\'')) || (v.starts_with('"') && v.ends_with('"')))
    {
        // Only keep the value when the text is a single literal.  An
        // expression that merely starts and ends with a quote (e.g. the
        // concatenation `'a' . 'b'`) still produces a string, but not
        // that literal.
        return if is_single_quoted_literal(v) {
            Some(PhpType::literal_string_raw(v.to_string()))
        } else {
            Some(PhpType::string())
        };
    }

    // Array literals.  A list built entirely from literals is a shape:
    // that is what makes `in_array($x, self::APPROVED, true)` and
    // `foreach (self::APPROVED as $entry)` see the values the constant
    // names rather than an unconstrained `array`.
    if let Some(inner) = strip_array_literal(v) {
        return Some(literal_array_shape(inner, elements).unwrap_or_else(PhpType::array));
    }

    let lower = v.to_lowercase();

    // Boolean / null keywords.
    if lower == "true" || lower == "false" {
        return Some(PhpType::bool());
    }
    if lower == "null" {
        return Some(PhpType::null());
    }

    // Numeric literals — try integer first, then float.
    // Strip optional leading sign for parsing.
    let (sign, numeric) = if let Some(rest) = v.strip_prefix('-') {
        ("-", rest)
    } else if let Some(rest) = v.strip_prefix('+') {
        ("", rest)
    } else {
        ("", v)
    };
    let int_literal = |raw: &str| {
        // Normalise hex/binary/octal/underscored spellings to the
        // decimal value (`0xFF` → `255`).  A spelling too large for an
        // `int` is a float, as PHP reads it.
        Some(match crate::php_type::parse_php_int_literal(raw) {
            Some(parsed) => PhpType::literal_int(parsed.to_string()),
            None => PhpType::float(),
        })
    };
    if numeric.starts_with("0x") || numeric.starts_with("0X") {
        // Hex integer.
        if numeric[2..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '_')
        {
            return int_literal(v);
        }
    }
    if numeric.starts_with("0b") || numeric.starts_with("0B") {
        // Binary integer.
        if numeric[2..]
            .chars()
            .all(|c| c == '0' || c == '1' || c == '_')
        {
            return int_literal(v);
        }
    }
    if numeric.starts_with("0o") || numeric.starts_with("0O") {
        // Octal integer (PHP 8.1+).
        if numeric[2..]
            .chars()
            .all(|c| ('0'..='7').contains(&c) || c == '_')
        {
            return int_literal(v);
        }
    }
    // Decimal integer (may contain underscores: 1_000_000).
    if !numeric.is_empty()
        && numeric.chars().all(|c| c.is_ascii_digit() || c == '_')
        && numeric.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return int_literal(v);
    }
    // Float: contains `.` or `e`/`E` among digits.
    if !numeric.is_empty() {
        let has_dot = numeric.contains('.');
        let has_exp = numeric.contains('e') || numeric.contains('E');
        if (has_dot || has_exp)
            && numeric.chars().all(|c| {
                c.is_ascii_digit()
                    || c == '.'
                    || c == 'e'
                    || c == 'E'
                    || c == '+'
                    || c == '-'
                    || c == '_'
            })
        {
            // The character check also accepts arithmetic like
            // `1.0-2.0`; only keep the value when the whole text
            // parses as one float.
            return Some(match crate::php_type::parse_php_float_literal(numeric) {
                Some(_) => PhpType::literal_float(format!("{sign}{numeric}")),
                None => PhpType::float(),
            });
        }
    }

    None
}

/// Whether `v` (which starts and ends with the same quote character) is
/// one string literal, i.e. its opening quote is closed only at the very
/// end.  `'foo'` → true, `'a' . 'b'` → false.
fn is_single_quoted_literal(v: &str) -> bool {
    let quote = v.as_bytes()[0];
    let inner = &v.as_bytes()[1..v.len() - 1];
    let mut i = 0;
    while i < inner.len() {
        match inner[i] {
            b'\\' => i += 2,
            b if b == quote => return false,
            _ => i += 1,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;

    #[test]
    fn constant_value_inference_keeps_scalar_literals() {
        assert_eq!(
            infer_type_from_constant_value("1"),
            Some(PhpType::literal_int("1"))
        );
        assert_eq!(
            infer_type_from_constant_value("-42"),
            Some(PhpType::literal_int("-42"))
        );
        assert_eq!(
            infer_type_from_constant_value("1_000"),
            Some(PhpType::literal_int("1000"))
        );
        assert_eq!(
            infer_type_from_constant_value("0xFF"),
            Some(PhpType::literal_int("255"))
        );
        // A sign in front of a non-decimal spelling keeps the value too.
        assert_eq!(
            infer_type_from_constant_value("-0xFF"),
            Some(PhpType::literal_int("-255"))
        );
        assert_eq!(
            infer_type_from_constant_value("-0b1010"),
            Some(PhpType::literal_int("-10"))
        );
        assert_eq!(
            infer_type_from_constant_value("+0o17"),
            Some(PhpType::literal_int("15"))
        );
        assert_eq!(
            infer_type_from_constant_value("-0_1_0"),
            Some(PhpType::literal_int("-8"))
        );
        assert_eq!(
            infer_type_from_constant_value("3.14"),
            Some(PhpType::literal_float("3.14"))
        );
        assert_eq!(
            infer_type_from_constant_value("-1.5e3"),
            Some(PhpType::literal_float("-1.5e3"))
        );
        assert_eq!(
            infer_type_from_constant_value("'foo'"),
            Some(PhpType::literal_string_raw("'foo'"))
        );
        assert_eq!(
            infer_type_from_constant_value("\"bar\""),
            Some(PhpType::literal_string_raw("\"bar\""))
        );
    }

    #[test]
    fn constant_value_inference_widens_non_literals() {
        // Concatenation that merely starts and ends with a quote.
        assert_eq!(
            infer_type_from_constant_value("'a' . 'b'"),
            Some(PhpType::string())
        );
        // A quote escaped at the end does not close the literal.
        assert_eq!(
            infer_type_from_constant_value("'it\\'s'"),
            Some(PhpType::literal_string_raw("'it\\'s'"))
        );
        // An integer literal too large for `int` is a float, as PHP reads it.
        assert_eq!(
            infer_type_from_constant_value("99999999999999999999"),
            Some(PhpType::float())
        );
        assert_eq!(
            infer_type_from_constant_value("0xFFFFFFFFFFFFFFFFF"),
            Some(PhpType::float())
        );
        // Float arithmetic passes the character filter but is not one value.
        assert_eq!(
            infer_type_from_constant_value("1.0-2.0"),
            Some(PhpType::float())
        );
        // Booleans and null keep their base type.
        assert_eq!(
            infer_type_from_constant_value("true"),
            Some(PhpType::bool())
        );
        assert_eq!(
            infer_type_from_constant_value("null"),
            Some(PhpType::null())
        );
        assert_eq!(infer_type_from_constant_value("self::OTHER"), None);
    }

    #[test]
    fn a_literal_array_constant_keeps_its_slots() {
        let entry = |key: Option<&str>, value_type: PhpType| crate::php_type::ShapeEntry {
            key: key.map(str::to_string),
            value_type,
            optional: false,
        };

        assert_eq!(
            infer_type_from_constant_value("[1, 2]"),
            Some(PhpType::list_shape(vec![
                entry(None, PhpType::literal_int("1")),
                entry(None, PhpType::literal_int("2")),
            ]))
        );
        // A trailing comma leaves no empty slot behind.
        assert_eq!(
            infer_type_from_constant_value("array('a', 'b',)"),
            Some(PhpType::list_shape(vec![
                entry(None, PhpType::literal_string_raw("'a'")),
                entry(None, PhpType::literal_string_raw("'b'")),
            ]))
        );
        assert_eq!(
            infer_type_from_constant_value("['name' => 'x', 3 => true]"),
            Some(PhpType::array_shape(vec![
                entry(Some("name"), PhpType::literal_string_raw("'x'")),
                entry(Some("3"), PhpType::bool()),
            ]))
        );
        assert_eq!(
            infer_type_from_constant_value("[]"),
            Some(PhpType::list_shape(vec![]))
        );
        // A `=>` inside a string is not a key separator.
        assert_eq!(
            infer_type_from_constant_value("['a => b']"),
            Some(PhpType::list_shape(vec![entry(
                None,
                PhpType::literal_string_raw("'a => b'")
            )]))
        );
        // An element that would have to be resolved leaves the whole
        // constant an unconstrained array rather than a shape missing a
        // slot.
        assert_eq!(
            infer_type_from_constant_value("[1, self::OTHER]"),
            Some(PhpType::array())
        );
        assert_eq!(
            infer_type_from_constant_value("[$key => 1]"),
            Some(PhpType::array())
        );
    }

    #[test]
    fn a_class_const_fetch_array_element_resolves_via_the_resolver() {
        let entry = |value_type: PhpType| crate::php_type::ShapeEntry {
            key: None,
            value_type,
            optional: false,
        };
        let resolve = |text: &str| -> Option<PhpType> {
            (text == "Foo::class").then(|| PhpType::class_string(Some(PhpType::named(atom("Foo")))))
        };

        // Without a resolver, `Foo::class` can't be classified and the
        // whole array falls back to unconstrained `array`.
        assert_eq!(
            infer_type_from_constant_value("[Foo::class]"),
            Some(PhpType::array())
        );

        // With one, the element becomes the literal class name PHPStan
        // itself reads a `::class` array element as.
        assert_eq!(
            infer_type_from_constant_value_resolved("[Foo::class]", &resolve),
            Some(PhpType::list_shape(vec![entry(
                PhpType::literal_string_value("Foo")
            )]))
        );

        // An element the resolver can't turn into a `class-string` still
        // leaves the whole array unconstrained, same as with no resolver.
        assert_eq!(
            infer_type_from_constant_value_resolved("[1, self::OTHER]", &resolve),
            Some(PhpType::array())
        );
    }
}
