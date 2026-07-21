//! Parsing, AST conversion, and type-operator evaluation.

use super::*;

impl PhpType {
    /// Parse a PHP type string into a structured [`PhpType`].
    ///
    /// This never fails. If the input cannot be parsed by `mago_type_syntax`,
    /// returns `PhpType::Raw(input)`.
    ///
    /// PHPStan/Larastan variance annotations (`covariant`, `contravariant`)
    /// inside generic parameter positions are stripped before parsing so
    /// that types like `BelongsTo<Category, covariant $this>` parse as
    /// `Generic("BelongsTo", [Named("Category"), Named("$this")])` instead
    /// of falling back to `Raw(…)`.
    pub fn parse(input: &str) -> PhpType {
        if input.is_empty() {
            return PhpType::Raw(String::new());
        }

        // Strip variance annotations that mago_type_syntax cannot parse.
        let cleaned = strip_variance_annotations_from_type(input);
        // Replace PHPStan `*` wildcards in generic positions with `mixed`.
        let cleaned = replace_star_wildcards(&cleaned);
        let effective: &str = &cleaned;

        let span = Span::new(
            FileId::zero(),
            Position::new(0),
            Position::new(effective.len() as u32),
        );

        let arena = LocalArena::new();
        let effective = arena.alloc_slice_copy(effective.as_bytes());
        // `mago-type-syntax` is deprecated in favour of `mago-phpdoc-syntax`;
        // the migration is tracked as a separate task.
        #[allow(deprecated)]
        let parsed = mago_type_syntax::parse_str(&arena, span, effective);
        match parsed {
            Ok(ty) => convert(&ty),
            Err(_) => PhpType::Raw(input.to_owned()),
        }
    }
}

pub(crate) fn literal_number_type(raw: String) -> PhpType {
    if parse_php_int_literal(&raw).is_some() {
        PhpType::literal_int(raw)
    } else {
        PhpType::literal_float(raw)
    }
}

pub(crate) fn parse_php_int_literal(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (negative, body) = if let Some(rest) = trimmed.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (false, rest)
    } else {
        (false, trimmed)
    };

    let clean = body.replace('_', "");
    let (radix, digits) = if let Some(rest) = clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = clean
        .strip_prefix("0b")
        .or_else(|| clean.strip_prefix("0B"))
    {
        (2, rest)
    } else if let Some(rest) = clean
        .strip_prefix("0o")
        .or_else(|| clean.strip_prefix("0O"))
    {
        (8, rest)
    } else if clean.len() > 1
        && clean.starts_with('0')
        && clean.bytes().all(|b| (b'0'..=b'7').contains(&b))
    {
        (8, &clean[1..])
    } else {
        (10, clean.as_str())
    };

    if digits.is_empty() {
        return None;
    }

    let unsigned = i64::from_str_radix(digits, radix).ok()?;
    if negative {
        unsigned.checked_neg()
    } else {
        Some(unsigned)
    }
}

pub(crate) fn parse_php_float_literal(raw: &str) -> Option<f64> {
    let clean = raw.trim().replace('_', "");
    if clean.is_empty() {
        return None;
    }
    clean.parse::<f64>().ok()
}

/// Replace PHPStan `*` wildcards in generic type argument positions with
/// `mixed`.
///
/// PHPStan's phpdoc-parser supports `*` as a bivariant wildcard inside
/// generic angle brackets, e.g. `Relation<TRelatedModel, *, *>`.  The
/// `*` simply means "any type" and is equivalent to `mixed`.
/// `mago-type-syntax` does not support this syntax, so we pre-process it.
///
/// Only replaces `*` tokens that appear inside angle brackets at generic
/// argument boundaries: preceded (ignoring whitespace) by `<` or `,` and
/// followed (ignoring whitespace) by `,` or `>`.  This avoids mangling:
/// - `Foo::*` — member references (preceded by `::`)
/// - `int-mask-of<self::FOO_*>` — constant wildcard patterns (preceded
///   by `_` or identifier chars)
///
/// Returns the input unchanged (no allocation) when no wildcards are found.
pub(crate) fn replace_star_wildcards(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('*') {
        return std::borrow::Cow::Borrowed(s);
    }

    let bytes = s.as_bytes();

    // First pass: check if any `*` is actually a generic wildcard.
    let has_generic_wildcard =
        (0..bytes.len()).any(|i| bytes[i] == b'*' && is_generic_wildcard(bytes, i));

    if !has_generic_wildcard {
        return std::borrow::Cow::Borrowed(s);
    }

    let mut result = String::with_capacity(s.len() + 16);
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'*' && is_generic_wildcard(bytes, i) {
            result.push_str("mixed");
            i += 1;
        } else {
            // Copy the whole UTF-8 character, not a single byte, so
            // multibyte characters in the type string are not mangled.
            let ch = s[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
    }

    std::borrow::Cow::Owned(result)
}

/// Check whether the `*` at position `pos` in `bytes` is a PHPStan
/// generic wildcard (as opposed to a member reference like `Foo::*`
/// or a constant pattern like `self::FOO_*`).
///
/// A generic wildcard `*` is preceded (ignoring whitespace) by `<` or
/// `,` and followed (ignoring whitespace) by `,` or `>`.
pub(crate) fn is_generic_wildcard(bytes: &[u8], pos: usize) -> bool {
    // Check preceding non-whitespace character.
    let prev_ok = {
        let mut j = pos;
        loop {
            if j == 0 {
                break false;
            }
            j -= 1;
            if !bytes[j].is_ascii_whitespace() {
                break bytes[j] == b'<' || bytes[j] == b',';
            }
        }
    };

    if !prev_ok {
        return false;
    }

    // Check following non-whitespace character.
    let mut k = pos + 1;
    while k < bytes.len() {
        if !bytes[k].is_ascii_whitespace() {
            return bytes[k] == b',' || bytes[k] == b'>';
        }
        k += 1;
    }

    false
}

/// Strip `covariant ` and `contravariant ` prefixes from generic type
/// arguments so that `mago_type_syntax` can parse the type.
///
/// Only strips the keywords when they appear immediately after `<` or `,`
/// (with optional whitespace), i.e. inside generic parameter positions.
/// Returns the input unchanged (no allocation) when no annotations are
/// found.
pub(crate) fn strip_variance_annotations_from_type(s: &str) -> std::borrow::Cow<'_, str> {
    // Fast path: no variance annotations at all.
    if !s.contains("covariant ") && !s.contains("contravariant ") {
        return std::borrow::Cow::Borrowed(s);
    }

    let mut cleaned = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        // Check whether the preceding non-whitespace is `<` or `,`,
        // meaning we are inside a generic parameter position.
        let preceded_by_generic_delimiter = || -> bool {
            let mut j = i;
            while j > 0 {
                j -= 1;
                if !bytes[j].is_ascii_whitespace() {
                    return bytes[j] == b'<' || bytes[j] == b',';
                }
            }
            false
        };

        if i + "covariant ".len() <= bytes.len()
            && &bytes[i..i + "covariant ".len()] == b"covariant "
            && preceded_by_generic_delimiter()
        {
            i += "covariant ".len();
        } else if i + "contravariant ".len() <= bytes.len()
            && &bytes[i..i + "contravariant ".len()] == b"contravariant "
            && preceded_by_generic_delimiter()
        {
            i += "contravariant ".len();
        } else {
            // Copy the whole UTF-8 character so multibyte characters in the
            // type string survive intact.
            let ch = s[i..].chars().next().unwrap();
            cleaned.push(ch);
            i += ch.len_utf8();
        }
    }

    std::borrow::Cow::Owned(cleaned)
}

/// Convert a borrowed mago AST `Type` into an owned `PhpType`.
pub(crate) fn convert(ty: &cst::Type<'_>) -> PhpType {
    match ty {
        // -- Composite types --------------------------------------------------
        cst::Type::Union(_) => {
            let members = flatten_union(ty);
            PhpType::Union(members)
        }
        cst::Type::Intersection(_) => {
            let members = flatten_intersection(ty);
            PhpType::Intersection(members)
        }
        cst::Type::Nullable(n) => PhpType::Nullable(Box::new(convert(n.inner))),
        cst::Type::Parenthesized(p) => convert(p.inner),

        // -- Named / Reference types ------------------------------------------
        cst::Type::Reference(r) => {
            let name = bytes_to_str(r.identifier.value).to_string();
            match &r.parameters {
                Some(params) => {
                    let mut args: Vec<PhpType> =
                        params.entries.iter().map(|e| convert(&e.inner)).collect();
                    // PHPStan's `__benevolent<T>` wrapper marks a lenient
                    // union; it is never a class. Treat the type as its
                    // inner `T`.
                    if args.len() == 1 && name == "__benevolent" {
                        return args.pop().unwrap();
                    }
                    PhpType::Generic(name, args)
                }
                None => PhpType::Named(name),
            }
        }

        // -- Array-like types with optional generic parameters ----------------
        cst::Type::Array(a) => {
            convert_keyword_with_optional_generics(bytes_to_str(a.keyword.value), &a.parameters)
        }
        cst::Type::NonEmptyArray(a) => {
            convert_keyword_with_optional_generics(bytes_to_str(a.keyword.value), &a.parameters)
        }
        cst::Type::AssociativeArray(a) => {
            convert_keyword_with_optional_generics(bytes_to_str(a.keyword.value), &a.parameters)
        }
        cst::Type::List(l) => {
            convert_keyword_with_optional_generics(bytes_to_str(l.keyword.value), &l.parameters)
        }
        cst::Type::NonEmptyList(l) => {
            convert_keyword_with_optional_generics(bytes_to_str(l.keyword.value), &l.parameters)
        }
        cst::Type::Iterable(i) => {
            convert_keyword_with_optional_generics(bytes_to_str(i.keyword.value), &i.parameters)
        }

        // -- Slice: T[] -------------------------------------------------------
        cst::Type::Slice(s) => PhpType::Array(Box::new(convert(s.inner))),

        // -- Shape types ------------------------------------------------------
        cst::Type::Shape(s) => {
            let entries: Vec<ShapeEntry> = s
                .fields
                .iter()
                .map(|field| {
                    let key = field.key.as_ref().map(|k| k.key.to_string());
                    let optional = field.is_optional();
                    let value_type = convert(field.value);
                    ShapeEntry {
                        key,
                        value_type,
                        optional,
                    }
                })
                .collect();

            match s.kind {
                cst::ShapeTypeKind::Array
                | cst::ShapeTypeKind::NonEmptyArray
                | cst::ShapeTypeKind::AssociativeArray
                | cst::ShapeTypeKind::List
                | cst::ShapeTypeKind::NonEmptyList => PhpType::ArrayShape(entries),
            }
        }

        // -- Object type (with optional shape) --------------------------------
        cst::Type::Object(o) => match &o.properties {
            Some(props) => {
                let entries: Vec<ShapeEntry> = props
                    .fields
                    .iter()
                    .map(|field| {
                        let key = field.key.as_ref().map(|k| k.key.to_string());
                        let optional = field.is_optional();
                        let value_type = convert(field.value);
                        ShapeEntry {
                            key,
                            value_type,
                            optional,
                        }
                    })
                    .collect();
                PhpType::ObjectShape(entries)
            }
            None => PhpType::object(),
        },

        // -- Callable types ---------------------------------------------------
        cst::Type::Callable(c) => {
            let kind = bytes_to_str(c.keyword.value).to_string();
            match &c.specification {
                Some(spec) => {
                    let params: Vec<CallableParam> = spec
                        .parameters
                        .entries
                        .iter()
                        .map(|p| {
                            let type_hint = match &p.parameter_type {
                                Some(t) => convert(t),
                                None => PhpType::mixed(),
                            };
                            CallableParam {
                                type_hint,
                                optional: p.is_optional(),
                                variadic: p.is_variadic(),
                            }
                        })
                        .collect();
                    let return_type = spec
                        .return_type
                        .as_ref()
                        .map(|rt| Box::new(convert(rt.return_type)));
                    PhpType::Callable {
                        kind,
                        params,
                        return_type,
                    }
                }
                None => PhpType::Named(kind),
            }
        }

        // -- Conditional types ------------------------------------------------
        cst::Type::Conditional(c) => PhpType::Conditional {
            param: c.subject.to_string(),
            negated: c.is_negated(),
            condition: Box::new(convert(c.target)),
            then_type: Box::new(convert(c.then)),
            else_type: Box::new(convert(c.otherwise)),
        },

        // -- class-string / interface-string ----------------------------------
        cst::Type::ClassString(c) => {
            let inner = c
                .parameter
                .as_ref()
                .map(|p| Box::new(convert(&p.entry.inner)));
            PhpType::ClassString(inner)
        }
        cst::Type::InterfaceString(i) => {
            let inner = i
                .parameter
                .as_ref()
                .map(|p| Box::new(convert(&p.entry.inner)));
            PhpType::InterfaceString(inner)
        }

        // -- key-of / value-of ------------------------------------------------
        cst::Type::KeyOf(k) => PhpType::KeyOf(Box::new(convert(&k.parameter.entry.inner))),
        cst::Type::ValueOf(v) => PhpType::ValueOf(Box::new(convert(&v.parameter.entry.inner))),

        // -- int range --------------------------------------------------------
        cst::Type::IntRange(r) => PhpType::IntRange(r.min.to_string(), r.max.to_string()),

        // -- Index access: T[K] -----------------------------------------------
        cst::Type::IndexAccess(i) => {
            PhpType::IndexAccess(Box::new(convert(i.target)), Box::new(convert(i.index)))
        }

        // -- Variable (e.g. $this in conditional types) -----------------------
        cst::Type::Variable(v) => PhpType::Named(bytes_to_str(v.value).to_string()),

        // -- Literal types ----------------------------------------------------
        cst::Type::LiteralInt(l) => PhpType::literal_int(bytes_to_str(l.raw).to_string()),
        cst::Type::LiteralFloat(l) => PhpType::literal_float(bytes_to_str(l.raw).to_string()),
        cst::Type::LiteralString(l) => PhpType::literal_string_raw(bytes_to_str(l.raw).to_string()),

        // -- Negated / Posited literals (e.g. -42, +42) -----------------------
        cst::Type::Negated(n) => literal_number_type(format!("-{}", n.number)),
        cst::Type::Posited(p) => literal_number_type(format!("+{}", p.number)),

        // -- Keyword types → Named -------------------------------------------
        cst::Type::Mixed(k)
        | cst::Type::NonEmptyMixed(k)
        | cst::Type::Null(k)
        | cst::Type::Void(k)
        | cst::Type::Never(k)
        | cst::Type::Resource(k)
        | cst::Type::ClosedResource(k)
        | cst::Type::OpenResource(k)
        | cst::Type::True(k)
        | cst::Type::False(k)
        | cst::Type::Bool(k)
        | cst::Type::Float(k)
        | cst::Type::Int(k)
        | cst::Type::PositiveInt(k)
        | cst::Type::NegativeInt(k)
        | cst::Type::NonPositiveInt(k)
        | cst::Type::NonNegativeInt(k)
        | cst::Type::NonZeroInt(k)
        | cst::Type::String(k)
        | cst::Type::StringableObject(k)
        | cst::Type::ArrayKey(k)
        | cst::Type::Numeric(k)
        | cst::Type::Scalar(k)
        | cst::Type::CallableString(k)
        | cst::Type::LowercaseCallableString(k)
        | cst::Type::UppercaseCallableString(k)
        | cst::Type::NumericString(k)
        | cst::Type::NonEmptyString(k)
        | cst::Type::NonEmptyLowercaseString(k)
        | cst::Type::LowercaseString(k)
        | cst::Type::NonEmptyUppercaseString(k)
        | cst::Type::UppercaseString(k)
        | cst::Type::TruthyString(k)
        | cst::Type::NonFalsyString(k)
        | cst::Type::UnspecifiedLiteralInt(k)
        | cst::Type::UnspecifiedLiteralString(k)
        | cst::Type::UnspecifiedLiteralFloat(k)
        | cst::Type::NonEmptyUnspecifiedLiteralString(k) => {
            PhpType::Named(normalize_keyword_casing(bytes_to_str(k.value)))
        }

        // -- Catch-all for anything else (non_exhaustive) ---------------------
        other => PhpType::Raw(other.to_string()),
    }
}

/// Convert a keyword type that has optional generic parameters (like
/// `array`, `array<int>`, `list<string>`, `non-empty-array<int, string>`,
/// `iterable<K, V>`).
pub(crate) fn convert_keyword_with_optional_generics(
    keyword: &str,
    parameters: &Option<cst::GenericParameters<'_>>,
) -> PhpType {
    let canonical = normalize_keyword_casing(keyword);
    match parameters {
        Some(params) => {
            let args: Vec<PhpType> = params.entries.iter().map(|e| convert(&e.inner)).collect();
            PhpType::Generic(canonical, args)
        }
        None => PhpType::Named(canonical),
    }
}

/// Recursively flatten a left-leaning binary union tree into a flat `Vec`.
pub(crate) fn flatten_union(ty: &cst::Type<'_>) -> Vec<PhpType> {
    match ty {
        cst::Type::Union(u) => {
            let mut types = flatten_union(u.left);
            types.extend(flatten_union(u.right));
            types
        }
        other => vec![convert(other)],
    }
}

/// Recursively flatten a left-leaning binary intersection tree into a flat `Vec`.
pub(crate) fn flatten_intersection(ty: &cst::Type<'_>) -> Vec<PhpType> {
    match ty {
        cst::Type::Intersection(i) => {
            let mut types = flatten_intersection(i.left);
            types.extend(flatten_intersection(i.right));
            types
        }
        other => vec![convert(other)],
    }
}

// ---------------------------------------------------------------------------
// Type operator evaluation (key-of, value-of, T[K])
// ---------------------------------------------------------------------------

/// Evaluate `key-of<T>` when `T` is a concrete array or shape type.
///
/// - `key-of<array{a: int, b: string}>` → `'a'|'b'`
/// - `key-of<array<string, mixed>>` → `string`
/// - `key-of<list<T>>` → `int`
/// - Otherwise returns `key-of<T>` unchanged.
pub(crate) fn evaluate_key_of(resolved: &PhpType) -> PhpType {
    match resolved {
        PhpType::ArrayShape(entries) => {
            let keys: Vec<PhpType> = entries
                .iter()
                .filter_map(|e| e.key.as_ref())
                .map(PhpType::literal_string_value)
                .collect();
            match keys.len() {
                0 => PhpType::Named("never".to_string()),
                1 => keys.into_iter().next().unwrap(),
                _ => PhpType::Union(keys),
            }
        }
        PhpType::Generic(name, args) => {
            let n = name.to_ascii_lowercase();
            match n.as_str() {
                "array" | "non-empty-array" if args.len() == 2 => args[0].clone(),
                "array" | "non-empty-array" if args.len() == 1 => PhpType::Named("int".to_string()),
                "list" | "non-empty-list" => PhpType::Named("int".to_string()),
                _ => PhpType::KeyOf(Box::new(resolved.clone())),
            }
        }
        PhpType::Array(_) => PhpType::Named("int".to_string()),
        _ => PhpType::KeyOf(Box::new(resolved.clone())),
    }
}

/// Evaluate `value-of<T>` when `T` is a concrete array or shape type.
///
/// - `value-of<array{a: int, b: string}>` → `int|string`
/// - `value-of<array<string, User>>` → `User`
/// - Otherwise returns `value-of<T>` unchanged.
pub(crate) fn evaluate_value_of(resolved: &PhpType) -> PhpType {
    match resolved {
        PhpType::ArrayShape(entries) => {
            let mut values: Vec<PhpType> = entries.iter().map(|e| e.value_type.clone()).collect();
            // Deduplicate the whole value set (not just adjacent duplicates),
            // so `array{a: int, b: string, c: int}` yields `int|string`.
            dedup_types(&mut values);
            match values.len() {
                0 => PhpType::Named("never".to_string()),
                1 => values.into_iter().next().unwrap(),
                _ => PhpType::Union(values),
            }
        }
        PhpType::Generic(name, args) => {
            let n = name.to_ascii_lowercase();
            match n.as_str() {
                "array" | "non-empty-array" if args.len() == 2 => args[1].clone(),
                "array" | "non-empty-array" | "list" | "non-empty-list" if args.len() == 1 => {
                    args[0].clone()
                }
                _ => PhpType::ValueOf(Box::new(resolved.clone())),
            }
        }
        PhpType::Array(inner) => *inner.clone(),
        _ => PhpType::ValueOf(Box::new(resolved.clone())),
    }
}

/// Evaluate indexed access `T[K]` when both operands are concrete.
///
/// - `array{a: int, b: string}['a']` → `int`
/// - `array{a: int, b: string}[key-of<...>]` → `int|string` (all values)
/// - Otherwise returns `T[K]` unchanged.
pub(crate) fn evaluate_index_access(base: &PhpType, index: &PhpType) -> PhpType {
    if let PhpType::ArrayShape(entries) = base {
        // If index is a literal string key, look it up directly.
        if let Some(bare_key) = literal_or_named_shape_key(index) {
            for entry in entries {
                if entry.key.as_deref() == Some(bare_key.as_str()) {
                    return entry.value_type.clone();
                }
            }
        }
        // If index is a union of literals, collect their value types.
        if let PhpType::Union(members) = index {
            let mut values: Vec<PhpType> = Vec::new();
            for member in members {
                if let Some(bare_key) = literal_or_named_shape_key(member) {
                    for entry in entries {
                        if entry.key.as_deref() == Some(bare_key.as_str())
                            && !values.contains(&entry.value_type)
                        {
                            values.push(entry.value_type.clone());
                        }
                    }
                }
            }
            if !values.is_empty() {
                return match values.len() {
                    1 => values.into_iter().next().unwrap(),
                    _ => PhpType::Union(values),
                };
            }
        }
    }
    // Generic array: T[K] where T is array<K2, V> → V
    if let PhpType::Generic(name, args) = base {
        let n = name.to_ascii_lowercase();
        match n.as_str() {
            "array" | "non-empty-array" if args.len() == 2 => return args[1].clone(),
            "array" | "non-empty-array" | "list" | "non-empty-list" if args.len() == 1 => {
                return args[0].clone();
            }
            _ => {}
        }
    }
    PhpType::IndexAccess(Box::new(base.clone()), Box::new(index.clone()))
}

pub(crate) fn literal_or_named_shape_key(ty: &PhpType) -> Option<String> {
    match ty {
        PhpType::Literal(lit) => lit.string_content().map(ToOwned::to_owned),
        PhpType::Named(key) => Some(key.clone()),
        _ => None,
    }
}
