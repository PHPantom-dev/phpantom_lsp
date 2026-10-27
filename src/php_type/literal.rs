//! Literal values (`42`, `1.5`, `'foo'`) as they appear in a type, and
//! PHP's runtime reading of them.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LiteralValue {
    Int(Box<str>),
    Float(Box<str>),
    String(Box<str>),
}

impl LiteralValue {
    pub fn int(raw: impl Into<Box<str>>) -> Self {
        Self::Int(raw.into())
    }

    pub fn float(raw: impl Into<Box<str>>) -> Self {
        Self::Float(raw.into())
    }

    pub fn string_raw(raw: impl Into<Box<str>>) -> Self {
        Self::String(raw.into())
    }

    pub fn string_value(value: impl AsRef<str>) -> Self {
        Self::String(
            format!(
                "'{}'",
                value.as_ref().replace('\\', "\\\\").replace('\'', "\\'")
            )
            .into(),
        )
    }

    pub fn as_raw(&self) -> String {
        match self {
            LiteralValue::Int(raw) | LiteralValue::Float(raw) | LiteralValue::String(raw) => {
                raw.to_string()
            }
        }
    }

    /// Return the runtime value of a string literal, decoding quote-specific
    /// escapes (`'x\\y'` and `"x\y"` both yield `x\y`) so callers can compare
    /// or resolve by the literal's actual value rather than its spelling.
    pub fn string_content(&self) -> Option<Cow<'_, str>> {
        let LiteralValue::String(raw) = self else {
            return None;
        };
        Some(
            crate::text_scan::decode_php_string_literal(raw).unwrap_or_else(|| {
                Cow::Borrowed(crate::text_scan::unquote_php_string(raw).unwrap_or(raw))
            }),
        )
    }

    pub fn parse_i64(&self) -> Option<i64> {
        match self {
            LiteralValue::Int(raw) => parse_php_int_literal(raw),
            _ => None,
        }
    }

    pub fn parse_f64(&self) -> Option<f64> {
        match self {
            LiteralValue::Float(raw) => parse_php_float_literal(raw),
            _ => None,
        }
    }

    pub fn is_numeric_string(&self) -> bool {
        self.string_content()
            .is_some_and(|content| is_php_numeric_string(&content))
    }

    /// The number a numeric-string literal holds, as PHP reads it in an
    /// arithmetic context: `'1'` is `1`, `' 1.5 '` is `1.5`, and an integer
    /// spelling too large for `int` is a float. `None` for anything that is
    /// not a numeric string.
    pub fn numeric_string_value(&self) -> Option<LiteralValue> {
        let content = self.string_content()?;
        if !is_php_numeric_string(&content) {
            return None;
        }
        let trimmed = content.trim_matches(|c: char| c.is_ascii_whitespace());
        if !trimmed.contains(['.', 'e', 'E'])
            && let Ok(value) = trimmed.parse::<i64>()
        {
            return Some(LiteralValue::int(value.to_string()));
        }
        let value = trimmed.parse::<f64>().ok().filter(|v| v.is_finite())?;
        Some(LiteralValue::float(format!("{value:?}")))
    }

    /// Whether `===` holds between the two values, whatever their spelling
    /// (`'a'` and `"a"`, `16` and `0x10`), or `None` when a literal's value
    /// cannot be read.
    pub fn identical_to(&self, other: &LiteralValue) -> Option<bool> {
        match (self, other) {
            (LiteralValue::Int(_), LiteralValue::Int(_)) => {
                Some(self.parse_i64()? == other.parse_i64()?)
            }
            (LiteralValue::Float(_), LiteralValue::Float(_)) => {
                Some(self.parse_f64()? == other.parse_f64()?)
            }
            (LiteralValue::String(_), LiteralValue::String(_)) => {
                Some(self.string_content()? == other.string_content()?)
            }
            _ => Some(false),
        }
    }

    /// Whether PHP 8's `==` holds between the two values, or `None` when a
    /// literal's value cannot be read.
    ///
    /// Two numeric strings, or a number and a numeric string, compare as
    /// numbers (`'1e3' == '1000'`); any other pair of strings compares
    /// byte for byte.  A number never equals a non-numeric string, since
    /// PHP 8 compares the two as strings and a number's string form is
    /// numeric.
    pub fn loosely_equals(&self, other: &LiteralValue) -> Option<bool> {
        let numeric = |value: &LiteralValue| -> Option<f64> {
            match value {
                LiteralValue::Int(_) => value.parse_i64().map(|n| n as f64),
                LiteralValue::Float(_) => value.parse_f64(),
                LiteralValue::String(_) => {
                    let content = value.string_content()?;
                    is_php_numeric_string(&content)
                        .then(|| content.trim().parse().ok())
                        .flatten()
                }
            }
        };
        match (self, other) {
            (LiteralValue::Int(_), LiteralValue::Int(_)) => {
                Some(self.parse_i64()? == other.parse_i64()?)
            }
            (LiteralValue::String(_), LiteralValue::String(_))
                if !self.is_numeric_string() || !other.is_numeric_string() =>
            {
                Some(self.string_content()? == other.string_content()?)
            }
            (LiteralValue::String(_), _) if !self.is_numeric_string() => Some(false),
            (_, LiteralValue::String(_)) if !other.is_numeric_string() => Some(false),
            _ => Some(numeric(self)? == numeric(other)?),
        }
    }
}

/// Match PHP 8's `NUM_STRING` grammar without accepting Rust-only float
/// spellings such as `NaN`/`inf`.
///
/// PHP allows surrounding ASCII whitespace, a sign, decimal forms such as
/// `.5`/`5.`, and an optional exponent. Source-literal features such as
/// underscores and hexadecimal prefixes are not part of runtime numeric
/// strings.
fn is_php_numeric_string(content: &str) -> bool {
    let bytes = content.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if start == end {
        return false;
    }

    let bytes = &bytes[start..end];
    let mut index = 0;
    if matches!(bytes[index], b'+' | b'-') {
        index += 1;
        if index == bytes.len() {
            return false;
        }
    }

    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let integer_digits = index - integer_start;

    let mut fractional_digits = 0;
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        fractional_digits = index - fraction_start;
    }
    if integer_digits == 0 && fractional_digits == 0 {
        return false;
    }

    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }

    index == bytes.len()
}

impl fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiteralValue::Int(raw) | LiteralValue::Float(raw) | LiteralValue::String(raw) => {
                write!(f, "{raw}")
            }
        }
    }
}
