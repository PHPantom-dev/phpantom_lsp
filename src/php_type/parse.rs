//! Parsing, AST conversion, and type-operator evaluation.

use std::collections::HashMap;

use mago_span::HasSpan;

use super::*;

impl PhpType {
    /// Parse a PHP type string into a structured [`PhpType`].
    ///
    /// This never fails. If the input cannot be parsed by
    /// `mago_phpdoc_syntax`, returns `PhpType::raw(input)`.
    ///
    /// PHPStan variance annotations (`covariant`, `contravariant`)
    /// inside generic parameter positions are parsed and discarded, so
    /// types like `BelongsTo<Category, covariant $this>` become
    /// `Generic("BelongsTo", [Named("Category"), Named("$this")])`.
    pub fn parse(input: &str) -> PhpType {
        Self::try_parse(input).unwrap_or_else(|| PhpType::raw(input))
    }

    /// Like [`PhpType::parse`], but returns `None` when `input` is not a
    /// valid type instead of keeping it as raw text.
    pub fn try_parse(input: &str) -> Option<PhpType> {
        if input.is_empty() {
            return Some(PhpType::untyped());
        }

        // `static(Foo)` / `$this(Foo)` is how a bounded late-static type is
        // displayed, and no PHPDoc grammar covers it, so each one is smuggled
        // past the parser as a placeholder name and put back afterwards.
        if input.contains('(')
            && let Some((cleaned, bounds)) = replace_late_static_bounds(input)
        {
            return Some(parse_type_text(&cleaned)?.substitute(&bounds));
        }

        parse_type_text(input)
    }
}

fn parse_type_text(input: &str) -> Option<PhpType> {
    // Replace known hyphenated pseudo-types (e.g. `model-property`)
    // with underscore placeholders so Mago can parse the surrounding
    // type structure.  The placeholders are restored in the result.
    let cleaned = replace_hyphenated_keywords(input);
    let effective: &str = &cleaned;

    let span = Span::new(
        FileId::zero(),
        Position::new(0),
        Position::new(effective.len() as u32),
    );

    let arena = LocalArena::new();
    match mago_phpdoc_syntax::parse_type(&arena, effective.as_bytes(), span) {
        Ok(ty) => Some(restore_hyphenated_keywords(convert(effective, &ty))),
        Err(_) => try_parse_hyphenated_generic(input),
    }
}

/// Placeholder stem standing in for a bounded late-static type while the
/// PHPDoc parser runs.  Each occurrence gets its own index so several bounds
/// in one type stay distinct.
const LATE_STATIC_PLACEHOLDER: &str = "__phpantom_late_static_";

/// Rewrite every `static(Some\Class)` / `$this(Some\Class)` in `input` as a
/// placeholder name the PHPDoc grammar accepts, returning the rewritten text
/// alongside the substitutions that turn the placeholders back into bounded
/// late-static types.
///
/// That notation is the only spelling those types have and no grammar covers
/// it, so without the detour the bound is dropped and an unresolved `static`
/// keyword comes back out of a value whose class was known.  Rewriting in
/// place rather than matching the whole input means a bound nested in a
/// generic argument (`list<static(App\Foo)>`, which `replace_self_bound`
/// produces) survives the round trip too.
///
/// The bound must be a single class name: `static(int)` and `static(Foo|Bar)`
/// are not shapes the display form ever produces, so they are left for the
/// ordinary parser rather than being invented here.  Returns `None` when the
/// input holds no such notation, which is the common case for a type that
/// merely contains a `callable(…)` shape.
fn replace_late_static_bounds(input: &str) -> Option<(String, HashMap<String, PhpType>)> {
    let mut bounds: HashMap<String, PhpType> = HashMap::new();
    let mut out = String::new();
    let mut cursor = 0;

    while let Some(open) = input[cursor..].find('(').map(|i| cursor + i) {
        let keyword = late_static_keyword(&input[..open]).filter(|&(start, _)| start >= cursor);
        let close = input[open + 1..].find(')').map(|i| open + 1 + i);
        let bound = close
            .map(|close| input[open + 1..close].trim())
            .filter(|b| {
                !b.is_empty()
                    && !is_keyword_type(b)
                    && b.chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
            });

        let (Some((start, is_this)), Some(close), Some(bound)) = (keyword, close, bound) else {
            out.push_str(&input[cursor..=open]);
            cursor = open + 1;
            continue;
        };

        out.push_str(&input[cursor..start]);
        let placeholder = format!("{LATE_STATIC_PLACEHOLDER}{}__", bounds.len());
        out.push_str(&placeholder);
        let bound_atom = atom(bound);
        bounds.insert(
            placeholder,
            if is_this {
                PhpType::this_type(bound_atom)
            } else {
                PhpType::static_type(bound_atom)
            },
        );
        cursor = close + 1;
    }

    if bounds.is_empty() {
        return None;
    }
    out.push_str(&input[cursor..]);
    Some((out, bounds))
}

/// If `head` ends with a bare `static` or `$this` keyword, report where that
/// keyword starts and whether it was `$this`.
fn late_static_keyword(head: &str) -> Option<(usize, bool)> {
    for (keyword, is_this) in [("static", false), ("$this", true)] {
        let Some(start) = head.len().checked_sub(keyword.len()) else {
            continue;
        };
        if !head.is_char_boundary(start) || &head[start..] != keyword {
            continue;
        }
        let runs_on_from_a_name = head[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '\\' | '$' | '-'));
        if !runs_on_from_a_name {
            return Some((start, is_this));
        }
    }
    None
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

/// Hyphenated PHPDoc pseudo-type names that `mago_phpdoc_syntax` cannot
/// parse because hyphens are not valid PHP identifier characters.
/// Each pair is `(hyphenated, placeholder)`.
const HYPHENATED_KEYWORDS: &[(&str, &str)] = &[("model-property", "__model_property__")];

/// Replace known hyphenated pseudo-type names with underscore
/// placeholders so that `mago_phpdoc_syntax` can parse the surrounding
/// type structure (e.g. `array<model-property<T>, mixed>`).
fn replace_hyphenated_keywords(s: &str) -> std::borrow::Cow<'_, str> {
    let mut result = std::borrow::Cow::Borrowed(s);
    for &(hyphenated, placeholder) in HYPHENATED_KEYWORDS {
        if result.contains(hyphenated) {
            result = std::borrow::Cow::Owned(result.replace(hyphenated, placeholder));
        }
    }
    result
}

/// Restore hyphenated pseudo-type names from underscore placeholders
/// in a parsed `PhpType` tree.
fn restore_hyphenated_keywords(ty: PhpType) -> PhpType {
    ty.resolve_names(&|name| {
        let mut result = name.to_string();
        for &(hyphenated, placeholder) in HYPHENATED_KEYWORDS {
            if result.contains(placeholder) {
                result = result.replace(placeholder, hyphenated);
            }
        }
        result
    })
}

/// Try to parse a hyphenated PHPDoc pseudo-type with generic arguments
/// that `mago_phpdoc_syntax` does not recognise (e.g. `model-property<T>`).
///
/// Hyphens are not valid PHP identifier characters, so the Mago parser
/// rejects them.  Known pseudo-types like `class-string` and
/// `non-empty-string` have dedicated CST nodes, but the ones the Laravel
/// PHPStan extensions add, like `model-property`, do not.  This function
/// acts as a fallback when
/// the Mago parse fails: it splits the input at the first `<` that
/// follows a hyphenated base name, parses the inner type(s) recursively,
/// and returns `TypeKind::Generic` if the base name is a recognised
/// keyword type.
fn try_parse_hyphenated_generic(input: &str) -> Option<PhpType> {
    let angle = input.find('<')?;
    let base = &input[..angle];
    if !base.contains('-') || !super::keywords::is_keyword_type(base) {
        return None;
    }
    let inner = input.get(angle + 1..input.len().checked_sub(1)?)?;
    if !input.ends_with('>') {
        return None;
    }
    let args: Vec<PhpType> = inner.split(',').map(|s| PhpType::parse(s.trim())).collect();
    Some(PhpType::generic(base, args))
}

/// The written name of a class-like reference.
///
/// `mago-phpdoc-syntax` distinguishes the relative keywords (`self`,
/// `static`, `parent`) from ordinary identifiers, but every consumer here
/// resolves them later against the enclosing class, so both forms collapse
/// to the source text.
pub(crate) fn reference_kind_name<'a>(kind: &cst::ReferenceKind<'a>) -> &'a str {
    match kind {
        cst::ReferenceKind::Identifier(identifier) => bytes_to_str(identifier.value),
        cst::ReferenceKind::Self_(keyword)
        | cst::ReferenceKind::Static(keyword)
        | cst::ReferenceKind::Parent(keyword) => bytes_to_str(keyword.value),
    }
}

/// The runtime array key a shape field names, decoded from `src`.
///
/// A quoted key is written in source spelling, and the parser hands back
/// only the text between the quotes: `array{'a\b': int}` and
/// `array{"a\\b": int}` both arrive as `a\b` even though the first keys on
/// `a\b` and the second on `a\b` for different reasons, and `array{"\x38":
/// int}` arrives as `\x38` where PHP keys on the integer `8`. Decoding the
/// literal the way PHP decodes it is what lets a later reader ask whether
/// the key is an integer one. Every other spelling (a bare identifier, an
/// integer, a class constant) already is the value it names.
fn shape_key_text(src: &str, key: &cst::ShapeKey<'_>) -> String {
    let span = key.span();
    src.get(span.start.offset as usize..span.end.offset as usize)
        .and_then(crate::text_scan::decode_php_string_literal)
        .map_or_else(|| key.to_string(), |decoded| decoded.into_owned())
}

/// Convert a borrowed mago AST `Type` into an owned `PhpType`.
fn convert(src: &str, ty: &cst::Type<'_>) -> PhpType {
    match ty {
        // -- Composite types --------------------------------------------------
        cst::Type::Union(_) => PhpType::union(flatten_union(src, ty)),
        cst::Type::Intersection(_) => PhpType::intersection(flatten_intersection(src, ty)),
        cst::Type::Nullable(n) => PhpType::nullable(convert(src, n.inner)),
        cst::Type::Parenthesized(p) => convert(src, p.inner),

        // -- Named / Reference types ------------------------------------------
        cst::Type::Reference(r) => {
            let name = reference_kind_name(&r.kind);
            // A hyphenated spelling no keyword covers is a pseudo-type from
            // some analyzer's vocabulary, never a class name.  `mixed` is the
            // honest reading of a type we have no model for; keeping the
            // spelling would send it through class resolution and enforce it
            // literally, so nothing would ever match it.
            if is_unmodelled_pseudo_type(name) {
                return PhpType::mixed();
            }
            match &r.parameters {
                Some(params) => {
                    let mut args: Vec<PhpType> = params
                        .entries
                        .iter()
                        .map(|e| convert(src, &e.inner))
                        .collect();
                    // PHPStan's `__benevolent<T>` wrapper marks a lenient
                    // union; it is never a class. It reads and resolves as
                    // its inner `T`, with the leniency kept as a marker the
                    // compatibility check consults.
                    if args.len() == 1 && name == "__benevolent" {
                        return PhpType::benevolent(args.pop().unwrap());
                    }
                    PhpType::generic(name, args)
                }
                None => PhpType::named(atom(name)),
            }
        }

        // -- Array-like types with optional generic parameters ----------------
        cst::Type::Array(a) => convert_keyword_with_optional_generics(
            src,
            bytes_to_str(a.keyword.value),
            &a.parameters,
        ),
        cst::Type::NonEmptyArray(a) => convert_keyword_with_optional_generics(
            src,
            bytes_to_str(a.keyword.value),
            &a.parameters,
        ),
        cst::Type::AssociativeArray(a) => convert_keyword_with_optional_generics(
            src,
            bytes_to_str(a.keyword.value),
            &a.parameters,
        ),
        cst::Type::List(l) => convert_keyword_with_optional_generics(
            src,
            bytes_to_str(l.keyword.value),
            &l.parameters,
        ),
        cst::Type::NonEmptyList(l) => convert_keyword_with_optional_generics(
            src,
            bytes_to_str(l.keyword.value),
            &l.parameters,
        ),
        cst::Type::Iterable(i) => convert_keyword_with_optional_generics(
            src,
            bytes_to_str(i.keyword.value),
            &i.parameters,
        ),

        // -- Slice: T[] -------------------------------------------------------
        cst::Type::Slice(s) => PhpType::array_of(convert(src, s.inner)),

        // -- Shape types ------------------------------------------------------
        cst::Type::Shape(s) => {
            let entries = shape_entries(src, s.fields.iter());

            match s.kind {
                cst::ShapeTypeKind::Array
                | cst::ShapeTypeKind::NonEmptyArray
                | cst::ShapeTypeKind::AssociativeArray => PhpType::array_shape(entries),
                cst::ShapeTypeKind::List | cst::ShapeTypeKind::NonEmptyList => {
                    PhpType::list_shape(entries)
                }
            }
        }

        // -- Object type (with optional shape) --------------------------------
        cst::Type::Object(o) => match &o.properties {
            Some(props) => {
                let entries = shape_entries(src, props.fields.iter());
                PhpType::object_shape(entries)
            }
            None => PhpType::object(),
        },

        // -- Callable types ---------------------------------------------------
        cst::Type::Callable(c) => {
            // `pure-callable` / `pure-closure` name a callable whose purity we
            // do not model; the callable part of them we do.
            let written = bytes_to_str(c.keyword.value);
            let kind = unmodelled_refinement_base(written).unwrap_or(written);
            match &c.specification {
                Some(spec) => {
                    let params: Vec<CallableParam> = spec
                        .parameters
                        .entries
                        .iter()
                        .map(|p| {
                            let type_hint = match &p.parameter_type {
                                Some(t) => convert(src, t),
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
                        .map(|rt| convert(src, rt.return_type));
                    PhpType::callable_spec(kind, params, return_type)
                }
                None => PhpType::named(atom(kind)),
            }
        }

        // -- Conditional types ------------------------------------------------
        cst::Type::Conditional(c) => PhpType::conditional(
            c.subject.to_string(),
            c.is_negated(),
            convert(src, c.target),
            convert(src, c.then),
            convert(src, c.r#else),
        ),

        // -- class-string / interface-string ----------------------------------
        cst::Type::ClassString(c) => {
            let inner = c.parameter.as_ref().map(|p| convert(src, &p.entry.inner));
            PhpType::class_string(inner)
        }
        cst::Type::InterfaceString(i) => {
            let inner = i.parameter.as_ref().map(|p| convert(src, &p.entry.inner));
            PhpType::interface_string(inner)
        }

        // -- key-of / value-of ------------------------------------------------
        // Evaluated as soon as the operand is concrete: `key-of<array{a: int}>`
        // is the union `'a'`, and only the unevaluated form (a class constant,
        // an unsubstituted template) survives as `KeyOf`/`ValueOf` for a later
        // pass to finish.
        cst::Type::KeyOf(k) => evaluate_key_of(&convert(src, &k.parameter.entry.inner)),
        cst::Type::ValueOf(v) => evaluate_value_of(&convert(src, &v.parameter.entry.inner)),

        // -- int range --------------------------------------------------------
        cst::Type::IntRange(r) => PhpType::int_range(r.min.to_string(), r.max.to_string()),

        // -- Index access: T[K] -----------------------------------------------
        cst::Type::IndexAccess(i) => {
            PhpType::index_access(convert(src, i.target), convert(src, i.index))
        }

        // -- Variable (e.g. $this in conditional types) -----------------------
        cst::Type::Variable(v) | cst::Type::ThisVariable(v) => {
            PhpType::named(atom(bytes_to_str(v.value)))
        }

        // -- Literal types ----------------------------------------------------
        cst::Type::LiteralInt(l) => PhpType::literal_int(bytes_to_str(l.raw)),
        cst::Type::LiteralFloat(l) => PhpType::literal_float(bytes_to_str(l.raw)),
        cst::Type::LiteralString(l) => PhpType::literal_string_raw(bytes_to_str(l.raw)),

        // -- Wildcard: PHPStan's bivariant `*` / `_` --------------------------
        // `Relation<TRelatedModel, *, *>` means "any type" in that generic
        // position, which is exactly `mixed` for our purposes.
        cst::Type::Wildcard(_) => PhpType::mixed(),

        // -- Negated / Posited literals (e.g. -42, +42) -----------------------
        cst::Type::Negated(n) => literal_number_type(format!("-{}", n.operand)),
        cst::Type::Posited(p) => literal_number_type(format!("+{}", p.operand)),

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
            let canonical = normalize_keyword_casing(bytes_to_str(k.value));
            match unmodelled_refinement_base(&canonical) {
                Some(base) => PhpType::named(atom(base)),
                None => PhpType::named(atom(&canonical)),
            }
        }

        // -- Catch-all for anything else (non_exhaustive) ---------------------
        other => PhpType::raw(other.to_string()),
    }
}

/// Convert a keyword type that has optional generic parameters (like
/// `array`, `array<int>`, `list<string>`, `non-empty-array<int, string>`,
/// `iterable<K, V>`).
fn convert_keyword_with_optional_generics(
    src: &str,
    keyword: &str,
    parameters: &Option<cst::GenericParameters<'_>>,
) -> PhpType {
    let canonical = normalize_keyword_casing(keyword);
    match parameters {
        Some(params) => {
            let args: Vec<PhpType> = params
                .entries
                .iter()
                .map(|e| convert(src, &e.inner))
                .collect();
            PhpType::generic(&canonical, args)
        }
        None => PhpType::named(atom(&canonical)),
    }
}

/// Convert the fields of an `array{…}` or `object{…}` shape to
/// [`ShapeEntry`] values.
fn shape_entries<'a, 'ast: 'a>(
    src: &str,
    fields: impl Iterator<Item = &'a cst::ShapeField<'ast>>,
) -> Vec<ShapeEntry> {
    fields
        .map(|field| ShapeEntry {
            key: field.key.as_ref().map(|k| shape_key_text(src, &k.key)),
            optional: field.is_optional(),
            value_type: convert(src, field.value),
        })
        .collect()
}

/// Recursively flatten a left-leaning binary union tree into a flat `Vec`.
fn flatten_union(src: &str, ty: &cst::Type<'_>) -> Vec<PhpType> {
    flatten_binary(src, ty, |ty| match ty {
        cst::Type::Union(u) => Some((u.left, u.right)),
        _ => None,
    })
}

/// Recursively flatten a left-leaning binary intersection tree into a flat `Vec`.
fn flatten_intersection(src: &str, ty: &cst::Type<'_>) -> Vec<PhpType> {
    flatten_binary(src, ty, |ty| match ty {
        cst::Type::Intersection(i) => Some((i.left, i.right)),
        _ => None,
    })
}

/// Flatten a left-leaning binary type tree, with `split` naming the two
/// sides of the node kind being flattened.
///
/// Anything `split` does not recognise is a leaf and is converted as it
/// stands.
fn flatten_binary<'ast>(
    src: &str,
    ty: &cst::Type<'ast>,
    split: impl Copy + Fn(&cst::Type<'ast>) -> Option<(&'ast cst::Type<'ast>, &'ast cst::Type<'ast>)>,
) -> Vec<PhpType> {
    match split(ty) {
        Some((left, right)) => {
            let mut types = flatten_binary(src, left, split);
            types.extend(flatten_binary(src, right, split));
            types
        }
        None => vec![convert(src, ty)],
    }
}

// ---------------------------------------------------------------------------
// Type operator evaluation (key-of, value-of, T[K])
// ---------------------------------------------------------------------------

/// Evaluate `key-of<T>` when `T` is a concrete array or shape type.
///
/// - `key-of<array{a: int, b: string}>` → `'a'|'b'`
/// - `key-of<array{'x', 5: 'y'}>` → `0|5`
/// - `key-of<array<string, mixed>>` → `string`
/// - `key-of<list<T>>` → `int`
/// - `key-of<list<int>|array{a: int}>` → `int|'a'`
/// - Otherwise returns `key-of<T>` unchanged.
pub(crate) fn evaluate_key_of(resolved: &PhpType) -> PhpType {
    match resolved.kind() {
        TypeKind::ArrayShape(entries) => {
            // An unkeyed entry sits at its position, and PHP stores a
            // decimal-integer string key as the integer itself, so both
            // come back as int literals.
            let mut keys: Vec<PhpType> = entries
                .iter()
                .enumerate()
                .map(|(position, e)| match e.key.as_deref() {
                    None => PhpType::literal_int(position.to_string()),
                    Some(key) if is_canonical_int_key(key) => PhpType::literal_int(key.to_string()),
                    Some(key) => PhpType::literal_string_value(key),
                })
                .collect();
            dedup_types(&mut keys);
            if keys.is_empty() {
                PhpType::named(atom("never"))
            } else {
                PhpType::union(keys)
            }
        }
        // `key-of<A|B>` is every key either member can have.  Only folded
        // when every member folds, so an operand still waiting on a
        // constant or template keeps the operator for a later pass.
        TypeKind::Union(members) => {
            let mut keys = Vec::with_capacity(members.len());
            for member in members {
                let key = evaluate_key_of(member);
                if matches!(key.kind(), TypeKind::KeyOf(_)) {
                    return PhpType::key_of(resolved.clone());
                }
                keys.push(key);
            }
            dedup_types(&mut keys);
            PhpType::union(keys)
        }
        TypeKind::Generic(g) => {
            let n = g.name.to_ascii_lowercase();
            match n.as_str() {
                "array" | "non-empty-array" if g.args.len() == 2 => g.args[0].clone(),
                "array" | "non-empty-array" if g.args.len() == 1 => PhpType::named(atom("int")),
                "list" | "non-empty-list" => PhpType::named(atom("int")),
                _ => PhpType::key_of(resolved.clone()),
            }
        }
        TypeKind::Array(_) => PhpType::named(atom("int")),
        _ => PhpType::key_of(resolved.clone()),
    }
}

/// Whether PHP stores an array key written as this string as an integer:
/// a decimal integer with no sign on zero, no leading zeros, and no
/// surrounding whitespace, within the platform integer range.
fn is_canonical_int_key(key: &str) -> bool {
    let digits = key.strip_prefix('-').unwrap_or(key);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" && key == "0" || !digits.starts_with('0'))
        && key.parse::<i64>().is_ok()
}

/// Evaluate `value-of<T>` when `T` is a concrete array or shape type.
///
/// - `value-of<array{a: int, b: string}>` → `int|string`
/// - `value-of<array<string, User>>` → `User`
/// - Otherwise returns `value-of<T>` unchanged.
pub(crate) fn evaluate_value_of(resolved: &PhpType) -> PhpType {
    match resolved.kind() {
        TypeKind::ArrayShape(entries) => {
            let mut values: Vec<PhpType> = entries.iter().map(|e| e.value_type.clone()).collect();
            // Deduplicate the whole value set (not just adjacent duplicates),
            // so `array{a: int, b: string, c: int}` yields `int|string`.
            dedup_types(&mut values);
            if values.is_empty() {
                PhpType::named(atom("never"))
            } else {
                PhpType::union(values)
            }
        }
        TypeKind::Generic(g) => {
            let n = g.name.to_ascii_lowercase();
            match n.as_str() {
                "array" | "non-empty-array" if g.args.len() == 2 => g.args[1].clone(),
                "array" | "non-empty-array" | "list" | "non-empty-list" if g.args.len() == 1 => {
                    g.args[0].clone()
                }
                _ => PhpType::value_of(resolved.clone()),
            }
        }
        TypeKind::Array(inner) => inner.clone(),
        _ => PhpType::value_of(resolved.clone()),
    }
}

/// Evaluate indexed access `T[K]` when both operands are concrete.
///
/// - `array{a: int, b: string}['a']` → `int`
/// - `array{a: int, b: string}[key-of<...>]` → `int|string` (all values)
/// - Otherwise returns `T[K]` unchanged.
pub(crate) fn evaluate_index_access(base: &PhpType, index: &PhpType) -> PhpType {
    if let TypeKind::ArrayShape(entries) = base.kind() {
        // If index is a literal string key, look it up directly.
        if let Some(bare_key) = literal_or_named_shape_key(index) {
            for entry in entries {
                if entry.key.as_deref() == Some(bare_key.as_str()) {
                    return entry.value_type.clone();
                }
            }
        }
        // If index is a union of literals, collect their value types.
        if let TypeKind::Union(members) = index.kind() {
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
                return PhpType::union(values);
            }
        }
    }
    // Generic array: T[K] where T is array<K2, V> → V
    if let TypeKind::Generic(g) = base.kind() {
        let n = g.name.to_ascii_lowercase();
        match n.as_str() {
            "array" | "non-empty-array" if g.args.len() == 2 => return g.args[1].clone(),
            "array" | "non-empty-array" | "list" | "non-empty-list" if g.args.len() == 1 => {
                return g.args[0].clone();
            }
            _ => {}
        }
    }
    PhpType::index_access(base.clone(), index.clone())
}

pub(crate) fn literal_or_named_shape_key(ty: &PhpType) -> Option<String> {
    match ty.kind() {
        TypeKind::Literal(lit) => lit.string_content().map(Cow::into_owned),
        TypeKind::Named(key) => Some(key.to_string()),
        _ => None,
    }
}
