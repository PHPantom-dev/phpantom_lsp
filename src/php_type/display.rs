//! `Display` implementations.

use super::*;

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for PhpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhpType::Named(s) => write!(f, "{s}"),

            PhpType::Nullable(inner) => write!(f, "?{inner}"),

            PhpType::Union(types) => {
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, "|")?;
                    }
                    // Wrap callable types in parentheses so
                    // `(Closure(int): string)|Foo` is not misread as
                    // `Closure(int): string|Foo`.
                    if matches!(ty, PhpType::Callable { .. }) {
                        write!(f, "({ty})")?;
                    } else {
                        write!(f, "{ty}")?;
                    }
                }
                Ok(())
            }

            PhpType::Intersection(types) => {
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, "&")?;
                    }
                    write!(f, "{ty}")?;
                }
                Ok(())
            }

            PhpType::Generic(name, args) => {
                write!(f, "{name}<")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ">")
            }

            PhpType::Array(inner) => {
                if inner.is_mixed() {
                    write!(f, "array")
                } else {
                    write!(f, "array<{inner}>")
                }
            }

            PhpType::ArrayShape(entries) => {
                write!(f, "array{{")?;
                for (i, entry) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{entry}")?;
                }
                write!(f, "}}")
            }

            PhpType::ObjectShape(entries) => {
                write!(f, "object{{")?;
                for (i, entry) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{entry}")?;
                }
                write!(f, "}}")
            }

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => {
                write!(f, "{kind}(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{param}")?;
                }
                write!(f, ")")?;
                if let Some(ret) = return_type {
                    write!(f, ": {ret}")?;
                }
                Ok(())
            }

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => {
                if *negated {
                    write!(f, "{param} is not {condition} ? {then_type} : {else_type}")
                } else {
                    write!(f, "{param} is {condition} ? {then_type} : {else_type}")
                }
            }

            PhpType::ClassString(inner) => match inner {
                Some(ty) => write!(f, "class-string<{ty}>"),
                None => write!(f, "class-string"),
            },

            PhpType::InterfaceString(inner) => match inner {
                Some(ty) => write!(f, "interface-string<{ty}>"),
                None => write!(f, "interface-string"),
            },

            PhpType::KeyOf(inner) => write!(f, "key-of<{inner}>"),

            PhpType::ValueOf(inner) => write!(f, "value-of<{inner}>"),

            PhpType::IntRange(min, max) => write!(f, "int<{min}..{max}>"),

            PhpType::IndexAccess(target, index) => write!(f, "{target}[{index}]"),

            PhpType::Literal(s) => write!(f, "{s}"),

            PhpType::Raw(s) => write!(f, "{s}"),
        }
    }
}

impl fmt::Display for ShapeEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            Some(key) => {
                let opt = if self.optional { "?" } else { "" };
                let formatted_key = format_shape_key(key);
                write!(f, "{formatted_key}{opt}: {}", self.value_type)
            }
            None => write!(f, "{}", self.value_type),
        }
    }
}

/// Format a shape key for display in a type string.
///
/// Keys that are simple identifiers (alphanumeric + underscore, not starting
/// with a digit) or plain integers are emitted bare.  Keys that contain
/// special characters (spaces, newlines, backslashes, colons, braces, quotes,
/// etc.) are wrapped in single quotes with `\` and `\n` / `\r` / `\t`
/// escaped so the type string remains a single readable line.
fn format_shape_key(key: &str) -> String {
    // Simple identifier-like keys: emit bare.
    let is_simple = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !key.starts_with(|c: char| c.is_ascii_digit());
    if is_simple {
        return key.to_string();
    }
    // Pure integer keys: emit bare.
    if key.parse::<i64>().is_ok() {
        return key.to_string();
    }
    // Quote and escape.
    let mut out = String::with_capacity(key.len() + 2);
    out.push('\'');
    for ch in key.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

impl fmt::Display for CallableParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.type_hint)?;
        if self.optional {
            write!(f, "=")?;
        } else if self.variadic {
            write!(f, "...")?;
        }
        Ok(())
    }
}
