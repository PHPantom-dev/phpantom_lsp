//! `Display` implementations.

use super::*;

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for PhpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.raw_kind() {
            // The leniency marker is a note about where the type came from,
            // not something a developer reading `string|false` should see.
            TypeKind::Benevolent(inner) => write!(f, "{inner}"),
            // The ordering promise is part of the type the developer wrote,
            // so it is spelled back the same way.
            TypeKind::ListShape(inner) => match inner.kind() {
                TypeKind::ArrayShape(entries) => write_shape(f, "list", entries),
                _ => write!(f, "{inner}"),
            },
            TypeKind::Named(s) => write!(f, "{s}"),
            TypeKind::StaticType(bound) => write!(f, "static({bound})"),
            TypeKind::ThisType(bound) => write!(f, "$this({bound})"),

            TypeKind::Nullable(inner) => write!(f, "?{inner}"),

            TypeKind::Union(types) => {
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, "|")?;
                    }
                    // Wrap callable types in parentheses so
                    // `(Closure(int): string)|Foo` is not misread as
                    // `Closure(int): string|Foo`.
                    if matches!(ty.kind(), TypeKind::Callable(_)) {
                        write!(f, "({ty})")?;
                    } else {
                        write!(f, "{ty}")?;
                    }
                }
                Ok(())
            }

            TypeKind::Intersection(types) => {
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, "&")?;
                    }
                    // `&` binds tighter than `|`, so a union member has to
                    // keep its parentheses: without them `(A|B)&C` reads
                    // back as `A|(B&C)`, which drops the intersection from
                    // the first branch.
                    if matches!(ty.kind(), TypeKind::Union(_) | TypeKind::Callable(_)) {
                        write!(f, "({ty})")?;
                    } else {
                        write!(f, "{ty}")?;
                    }
                }
                Ok(())
            }

            TypeKind::Generic(g) => {
                let name = &g.name;
                write!(f, "{name}<")?;
                for (i, arg) in g.args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ">")
            }

            TypeKind::Array(inner) => {
                if inner.is_mixed() {
                    write!(f, "array")
                } else {
                    write!(f, "array<{inner}>")
                }
            }

            TypeKind::ArrayShape(entries) => write_shape(f, "array", entries),

            TypeKind::ObjectShape(entries) => write_shape(f, "object", entries),

            TypeKind::Callable(c) => {
                let kind = &c.kind;
                write!(f, "{kind}(")?;
                for (i, param) in c.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{param}")?;
                }
                write!(f, ")")?;
                if let Some(ret) = &c.return_type {
                    write!(f, ": {ret}")?;
                }
                Ok(())
            }

            TypeKind::Conditional(c) => {
                let ConditionalType {
                    param,
                    negated,
                    condition,
                    then_type,
                    else_type,
                    // A modelling detail of how an undecided condition is
                    // answered, not part of the type's spelling.
                    else_when_undecided: _,
                } = &**c;
                if *negated {
                    write!(f, "{param} is not {condition} ? {then_type} : {else_type}")
                } else {
                    write!(f, "{param} is {condition} ? {then_type} : {else_type}")
                }
            }

            TypeKind::ClassString(inner) => match inner {
                Some(ty) => write!(f, "class-string<{ty}>"),
                None => write!(f, "class-string"),
            },

            TypeKind::InterfaceString(inner) => match inner {
                Some(ty) => write!(f, "interface-string<{ty}>"),
                None => write!(f, "interface-string"),
            },

            TypeKind::KeyOf(inner) => write!(f, "key-of<{inner}>"),

            TypeKind::ValueOf(inner) => write!(f, "value-of<{inner}>"),

            TypeKind::IntRange(min, max) => write!(f, "int<{min}, {max}>"),

            TypeKind::IndexAccess(target, index) => write!(f, "{target}[{index}]"),

            TypeKind::Literal(s) => write!(f, "{s}"),

            TypeKind::Raw(s) => write!(f, "{s}"),
        }
    }
}

/// Write `base{entry, entry, …}`, the form every shape type shares.
fn write_shape(f: &mut fmt::Formatter<'_>, base: &str, entries: &[ShapeEntry]) -> fmt::Result {
    write!(f, "{base}{{")?;
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{entry}")?;
    }
    write!(f, "}}")
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
/// etc.) are quoted.
///
/// The quoting has to be one PHP itself would decode back to the same key,
/// because a displayed type is re-parsed (through hover text, a `@var`
/// written into a file, a cached type string) and its keys are read as the
/// runtime values they name. Single quotes carry everything but the
/// control characters, which have no single-quoted spelling at all, so a
/// key holding one is emitted double-quoted instead of being written as an
/// escape PHP would read back as a literal backslash.
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

    let mut out = String::with_capacity(key.len() + 2);
    if key.contains(['\n', '\r', '\t']) {
        out.push('"');
        for ch in key.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '$' => out.push_str("\\$"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(ch),
            }
        }
        out.push('"');
        return out;
    }

    out.push('\'');
    for ch in key.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
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
