//! Structured representation of PHP type expressions.
//!
//! This module provides [`PhpType`], an owned enum that represents PHP type
//! expressions as a tree. It is converted from the borrowed
//! `mago_type_syntax::ast::Type<'input>` AST and can be displayed back into a
//! canonical string form.
//!
//! # Design
//!
//! `mago_type_syntax::ast::Type` is `#[non_exhaustive]` with 69 variants and
//! borrows from input. `PhpType` is simpler: keyword types are collapsed into
//! `Named`, generic-parameterised references become `Generic`, and rarely-used
//! variants fall back to `Raw`.
//!
//! `PhpType::parse()` never fails. If the input cannot be parsed or mapped,
//! it returns `PhpType::Raw(input)`.

use std::fmt;

use bumpalo::Bump;
use mago_database::file::FileId;
use mago_span::{Position, Span};
use mago_type_syntax::ast;

use crate::atom::bytes_to_str;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A structured, owned representation of a PHP type expression.
#[derive(Debug, Clone, PartialEq)]
pub enum PhpType {
    /// A named type: keywords (`int`, `string`, `mixed`, `void`, …),
    /// class references (`Foo\Bar`), or special names (`self`, `static`,
    /// `parent`). Also used for PHPDoc variable references (`$this`).
    Named(String),

    /// Nullable type: `?T`.
    Nullable(Box<PhpType>),

    /// Union type: `T|U|V`. Always contains two or more members.
    Union(Vec<PhpType>),

    /// Intersection type: `T&U`. Always contains two or more members.
    Intersection(Vec<PhpType>),

    /// Generic (parameterised) type: `Collection<int, User>`, `array<string>`,
    /// `list<int>`, `non-empty-array<string>`, `iterable<K, V>`, etc.
    Generic(String, Vec<PhpType>),

    /// The `T[]` slice syntax (sugar for `array<int, T>`).
    Array(Box<PhpType>),

    /// Array shape: `array{key: string, age?: int}`.
    ArrayShape(Vec<ShapeEntry>),

    /// Object shape: `object{name: string}`.
    ObjectShape(Vec<ShapeEntry>),

    /// Callable or Closure type with optional specification.
    /// `callable(int, string): bool`, `Closure(int): void`,
    /// `pure-callable(T): U`, `pure-Closure(T): U`.
    Callable {
        /// One of `"callable"`, `"Closure"`, `"pure-callable"`, `"pure-Closure"`.
        kind: String,
        /// Parameter types.
        params: Vec<CallableParam>,
        /// Optional return type.
        return_type: Option<Box<PhpType>>,
    },

    /// Conditional return type: `$x is T ? U : V`.
    Conditional {
        /// The subject (typically a variable like `$this`).
        param: String,
        /// Whether the condition is negated (`is not`).
        negated: bool,
        /// The condition type.
        condition: Box<PhpType>,
        /// The type when the condition is true.
        then_type: Box<PhpType>,
        /// The type when the condition is false.
        else_type: Box<PhpType>,
    },

    /// `class-string<T>` or bare `class-string`.
    ClassString(Option<Box<PhpType>>),

    /// `interface-string<T>` or bare `interface-string`.
    InterfaceString(Option<Box<PhpType>>),

    /// `key-of<T>`.
    KeyOf(Box<PhpType>),

    /// `value-of<T>`.
    ValueOf(Box<PhpType>),

    /// `int<min, max>` range type.
    IntRange(String, String),

    /// Index access type: `T[K]`.
    IndexAccess(Box<PhpType>, Box<PhpType>),

    /// A literal scalar type with preserved kind and source text.
    Literal(LiteralValue),

    /// Fallback for anything we cannot parse or do not yet map.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Int(String),
    Float(String),
    String(String),
}

impl LiteralValue {
    pub fn int(raw: impl Into<String>) -> Self {
        Self::Int(raw.into())
    }

    pub fn float(raw: impl Into<String>) -> Self {
        Self::Float(raw.into())
    }

    pub fn string_raw(raw: impl Into<String>) -> Self {
        Self::String(raw.into())
    }

    pub fn string_value(value: impl AsRef<str>) -> Self {
        Self::String(format!(
            "'{}'",
            value.as_ref().replace('\\', "\\\\").replace('\'', "\\'")
        ))
    }

    pub fn as_raw(&self) -> String {
        match self {
            LiteralValue::Int(raw) | LiteralValue::Float(raw) | LiteralValue::String(raw) => {
                raw.clone()
            }
        }
    }

    pub fn string_content(&self) -> Option<&str> {
        let LiteralValue::String(raw) = self else {
            return None;
        };
        crate::util::unquote_php_string(raw).or(Some(raw.as_str()))
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
        // Validate the string *content* as a PHP numeric string
        // (`is_numeric`), not as a PHP source literal.  Underscores,
        // hex/binary/octal prefixes, and leading `0` octal are only
        // meaningful in source code; the runtime string `'0xFF'` or
        // `'1_000'` is not numeric.
        self.string_content()
            .is_some_and(|content| content.parse::<i64>().is_ok() || content.parse::<f64>().is_ok())
    }
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

/// A single field in an array or object shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeEntry {
    /// The key name or integer index. `None` for positional (unkeyed) entries.
    pub key: Option<String>,
    /// The value type of this field.
    pub value_type: PhpType,
    /// Whether this field is optional (`key?: type`).
    pub optional: bool,
}

/// A single parameter in a callable type specification.
#[derive(Debug, Clone, PartialEq)]
pub struct CallableParam {
    /// The type of this parameter.
    pub type_hint: PhpType,
    /// Whether the parameter is optional (has `=`).
    pub optional: bool,
    /// Whether the parameter is variadic (`...`).
    pub variadic: bool,
}

// ---------------------------------------------------------------------------
// Convenience constructors for common keyword types
// ---------------------------------------------------------------------------

impl PhpType {
    /// `int` type.
    pub fn int() -> PhpType {
        PhpType::Named("int".to_owned())
    }

    /// `string` type.
    pub fn string() -> PhpType {
        PhpType::Named("string".to_owned())
    }

    /// `float` type.
    pub fn float() -> PhpType {
        PhpType::Named("float".to_owned())
    }

    /// `bool` type.
    pub fn bool() -> PhpType {
        PhpType::Named("bool".to_owned())
    }

    pub fn literal_int(raw: impl Into<String>) -> PhpType {
        PhpType::Literal(LiteralValue::int(raw))
    }

    pub fn literal_float(raw: impl Into<String>) -> PhpType {
        PhpType::Literal(LiteralValue::float(raw))
    }

    pub fn literal_string_raw(raw: impl Into<String>) -> PhpType {
        PhpType::Literal(LiteralValue::string_raw(raw))
    }

    pub fn literal_string_value(value: impl AsRef<str>) -> PhpType {
        PhpType::Literal(LiteralValue::string_value(value))
    }

    /// `true` type.
    pub fn true_() -> PhpType {
        PhpType::Named("true".to_owned())
    }

    /// `false` type.
    pub fn false_() -> PhpType {
        PhpType::Named("false".to_owned())
    }

    /// `null` type.
    pub fn null() -> PhpType {
        PhpType::Named("null".to_owned())
    }

    /// `void` type.
    pub fn void() -> PhpType {
        PhpType::Named("void".to_owned())
    }

    /// `mixed` type.
    pub fn mixed() -> PhpType {
        PhpType::Named("mixed".to_owned())
    }

    /// `never` type.
    pub fn never() -> PhpType {
        PhpType::Named("never".to_owned())
    }

    /// `array` type (bare, unparameterised).
    pub fn array() -> PhpType {
        PhpType::Named("array".to_owned())
    }

    /// `object` type.
    pub fn object() -> PhpType {
        PhpType::Named("object".to_owned())
    }

    /// `callable` type.
    pub fn callable() -> PhpType {
        PhpType::Named("callable".to_owned())
    }

    /// `\Closure` type (fully-qualified).
    pub fn closure() -> PhpType {
        PhpType::Named("Closure".to_string())
    }

    /// `iterable` type.
    pub fn iterable() -> PhpType {
        PhpType::Named("iterable".to_owned())
    }

    /// `self` type.
    pub fn self_() -> PhpType {
        PhpType::Named("self".to_owned())
    }

    /// `static` type.
    pub fn static_() -> PhpType {
        PhpType::Named("static".to_owned())
    }

    /// `$this` type.
    pub fn this() -> PhpType {
        PhpType::Named("$this".to_owned())
    }

    /// `parent` type.
    pub fn parent_() -> PhpType {
        PhpType::Named("parent".to_owned())
    }

    /// `numeric` pseudo-type.
    pub fn numeric() -> PhpType {
        PhpType::Named("numeric".to_owned())
    }

    /// Internal `__empty` sentinel used during type narrowing to represent
    /// a fully-filtered-out union member.
    pub fn empty_sentinel() -> PhpType {
        PhpType::Named("__empty".to_owned())
    }

    /// Convenience constructor for the "no type information" sentinel.
    ///
    /// Uses `Raw(String::new())` under the hood.  Prefer this over a bare
    /// `PhpType::Raw(String::new())` so the intent ("absence of type") is
    /// distinguishable from "unparseable input" at a glance.
    pub fn untyped() -> PhpType {
        PhpType::Raw(String::new())
    }

    /// Returns `true` when this value represents the "no type" sentinel
    /// produced by [`PhpType::untyped()`].
    pub fn is_untyped(&self) -> bool {
        matches!(self, PhpType::Raw(s) if s.is_empty())
    }

    /// `list<T>` generic type.
    pub fn list(elem: PhpType) -> PhpType {
        PhpType::Generic("list".to_owned(), vec![elem])
    }

    /// `array<K, V>` generic type with explicit key and value types.
    pub fn generic_array(key: PhpType, val: PhpType) -> PhpType {
        PhpType::Generic("array".to_owned(), vec![key, val])
    }

    /// `array<V>` generic type with only a value type (implicit integer key).
    pub fn generic_array_val(val: PhpType) -> PhpType {
        PhpType::Generic("array".to_owned(), vec![val])
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

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

        let arena = Bump::new();
        let effective = arena.alloc_slice_copy(effective.as_bytes());
        match mago_type_syntax::parse_str(&arena, span, effective) {
            Ok(ty) => convert(&ty),
            Err(_) => PhpType::Raw(input.to_owned()),
        }
    }

    /// Produce a new `PhpType` with all class names resolved through
    /// the provided callback.
    ///
    /// The callback receives each class-like name (from `Named`,
    /// `Generic`, `ClassString`, etc.) and returns the resolved
    /// fully-qualified name. Names that are keywords/scalars are
    /// never passed to the callback.
    ///
    /// This replaces the character-by-character `resolve_type_string`
    /// function in `ast_update.rs` with a clean tree traversal.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let ty = PhpType::parse("Collection<int, User>|null");
    /// let resolved = ty.resolve_names(&|name| {
    ///     use_map.get(name).cloned()
    ///         .unwrap_or_else(|| format!("App\\{}", name))
    /// });
    /// // → Generic("App\\Collection", [Named("int"), Named("App\\User")]) | Named("null")
    /// ```
    pub fn resolve_names(&self, resolver: &dyn Fn(&str) -> String) -> PhpType {
        match self {
            PhpType::Named(s) => {
                if is_keyword_type(s) {
                    PhpType::Named(s.clone())
                } else {
                    PhpType::Named(resolver(s))
                }
            }

            PhpType::Nullable(inner) => PhpType::Nullable(Box::new(inner.resolve_names(resolver))),

            PhpType::Union(types) => {
                PhpType::Union(types.iter().map(|t| t.resolve_names(resolver)).collect())
            }

            PhpType::Intersection(types) => {
                PhpType::Intersection(types.iter().map(|t| t.resolve_names(resolver)).collect())
            }

            PhpType::Generic(name, args) => {
                let resolved_name = if is_keyword_type(name) {
                    name.clone()
                } else {
                    resolver(name)
                };
                PhpType::Generic(
                    resolved_name,
                    args.iter().map(|a| a.resolve_names(resolver)).collect(),
                )
            }

            PhpType::Array(inner) => PhpType::Array(Box::new(inner.resolve_names(resolver))),

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.resolve_names(resolver),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.resolve_names(resolver),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: if is_keyword_type(kind) {
                    kind.clone()
                } else {
                    resolver(kind)
                },
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.resolve_names(resolver),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type
                    .as_ref()
                    .map(|rt| Box::new(rt.resolve_names(resolver))),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.resolve_names(resolver)),
                then_type: Box::new(then_type.resolve_names(resolver)),
                else_type: Box::new(else_type.resolve_names(resolver)),
            },

            PhpType::ClassString(inner) => {
                PhpType::ClassString(inner.as_ref().map(|i| Box::new(i.resolve_names(resolver))))
            }

            PhpType::InterfaceString(inner) => PhpType::InterfaceString(
                inner.as_ref().map(|i| Box::new(i.resolve_names(resolver))),
            ),

            PhpType::KeyOf(inner) => PhpType::KeyOf(Box::new(inner.resolve_names(resolver))),

            PhpType::ValueOf(inner) => PhpType::ValueOf(Box::new(inner.resolve_names(resolver))),

            PhpType::IntRange(min, max) => PhpType::IntRange(min.clone(), max.clone()),

            PhpType::IndexAccess(target, index) => PhpType::IndexAccess(
                Box::new(target.resolve_names(resolver)),
                Box::new(index.resolve_names(resolver)),
            ),

            PhpType::Literal(s) => PhpType::Literal(s.clone()),

            // Raw types can't be structurally resolved — pass through.
            PhpType::Raw(s) => PhpType::Raw(s.clone()),
        }
    }

    /// Return the short (unqualified) name from a potentially
    /// namespace-qualified type name. Returns only the part after the
    /// last `\`. Non-class types pass through unchanged.
    fn short_name_of(name: &str) -> &str {
        crate::util::short_name(name.trim())
    }

    /// Produce a new `PhpType` with all namespace-qualified names
    /// shortened to their unqualified form.
    ///
    /// For example, `App\Models\User|null` becomes `User|null`, and
    /// `array<int, App\Models\User>` becomes `array<int, User>`.
    pub fn shorten(&self) -> PhpType {
        match self {
            PhpType::Named(s) => PhpType::Named(Self::short_name_of(s).to_owned()),

            PhpType::Nullable(inner) => PhpType::Nullable(Box::new(inner.shorten())),

            PhpType::Union(types) => PhpType::Union(types.iter().map(|t| t.shorten()).collect()),

            PhpType::Intersection(types) => {
                PhpType::Intersection(types.iter().map(|t| t.shorten()).collect())
            }

            PhpType::Generic(name, args) => PhpType::Generic(
                Self::short_name_of(name).to_owned(),
                args.iter().map(|a| a.shorten()).collect(),
            ),

            PhpType::Array(inner) => PhpType::Array(Box::new(inner.shorten())),

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.shorten(),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.shorten(),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: Self::short_name_of(kind).to_owned(),
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.shorten(),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type.as_ref().map(|rt| Box::new(rt.shorten())),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.shorten()),
                then_type: Box::new(then_type.shorten()),
                else_type: Box::new(else_type.shorten()),
            },

            PhpType::ClassString(inner) => {
                PhpType::ClassString(inner.as_ref().map(|i| Box::new(i.shorten())))
            }

            PhpType::InterfaceString(inner) => {
                PhpType::InterfaceString(inner.as_ref().map(|i| Box::new(i.shorten())))
            }

            PhpType::KeyOf(inner) => PhpType::KeyOf(Box::new(inner.shorten())),

            PhpType::ValueOf(inner) => PhpType::ValueOf(Box::new(inner.shorten())),

            PhpType::IntRange(min, max) => PhpType::IntRange(min.clone(), max.clone()),

            PhpType::IndexAccess(target, index) => {
                PhpType::IndexAccess(Box::new(target.shorten()), Box::new(index.shorten()))
            }

            PhpType::Literal(s) => PhpType::Literal(s.clone()),

            PhpType::Raw(s) => {
                // Best-effort: apply the old string-based shortening
                // for raw types that we couldn't parse structurally.
                PhpType::Raw(s.clone())
            }
        }
    }

    /// Whether this type represents "no type" (an empty `Raw` or `Named`
    /// variant whose display string would be empty).
    ///
    /// This avoids the `.to_string().is_empty()` round-trip when callers
    /// only need to know whether a `PhpType` carries meaningful content.
    pub fn is_empty(&self) -> bool {
        matches!(self, PhpType::Raw(s) | PhpType::Named(s) if s.is_empty())
    }

    /// Whether this type is the internal `__empty` sentinel used during
    /// type narrowing to represent a fully-filtered-out union member.
    pub fn is_empty_sentinel(&self) -> bool {
        matches!(self, PhpType::Named(s) if s == "__empty")
    }

    /// Whether this type is a primitive scalar / built-in type that
    /// cannot have members accessed on it at runtime.
    ///
    /// Matches the narrow set of primitive PHP types:
    /// `int`, `float`, `string`, `bool`, `void`, `never`, `null`,
    /// `false`, `true`, `array`, `callable`, `iterable`, `resource`
    /// (and their aliases `integer`, `double`, `boolean`).
    ///
    /// Unlike [`is_scalar`], this does **not** include `mixed`, `object`,
    /// `class-string`, `self`, `static`, `parent`, or other PHPDoc
    /// pseudo-types on which member access may be valid.
    pub fn is_primitive_scalar(&self) -> bool {
        match self {
            PhpType::Named(s) => is_primitive_scalar_name(s),
            PhpType::Nullable(inner) => inner.is_primitive_scalar(),
            PhpType::Generic(name, _) => is_primitive_scalar_name(name),
            PhpType::Array(_) => true,
            PhpType::ArrayShape(_) => true,
            PhpType::Callable { .. } => true,
            PhpType::IntRange(_, _) => true,
            PhpType::Literal(_) => true,
            PhpType::Raw(_) => false,
            _ => false,
        }
    }

    /// Whether this type is a bare, unparameterised primitive scalar name.
    ///
    /// Returns `true` only for simple `PhpType::Named` values whose name
    /// is a primitive scalar keyword: `int`, `string`, `bool`, `void`,
    /// `null`, `array`, `callable`, `iterable`, `resource` (and aliases
    /// like `integer`, `double`, `boolean`).
    ///
    /// Returns `false` for:
    /// - PHPDoc pseudo-types (`non-empty-string`, `class-string`, `positive-int`)
    /// - Parameterised types (`array<int>`, `int<0, max>`, `list<User>`)
    /// - Shapes, callables with signatures, slices (`Foo[]`)
    /// - Class names, unions, intersections, nullable wrappers, etc.
    ///
    /// Use this when you need to detect that a docblock type is just a
    /// bare keyword that carries no extra information over a native hint.
    pub fn is_bare_primitive_scalar(&self) -> bool {
        matches!(self, PhpType::Named(s) if is_primitive_scalar_name(s))
    }

    /// Whether this type is a scalar/built-in type that does not refer
    /// to a user-defined class.
    ///
    /// Returns `true` when this type is exactly `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, PhpType::Named(s) if s.eq_ignore_ascii_case("null"))
    }

    /// Whether this type is `bool` or `boolean` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?bool` (nullable wrapper).
    pub fn is_bool(&self) -> bool {
        match self {
            PhpType::Named(s) => matches!(s.to_ascii_lowercase().as_str(), "bool" | "boolean"),
            PhpType::Nullable(inner) => inner.is_bool(),
            _ => false,
        }
    }

    /// Whether this type is `true` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?true` (nullable wrapper).
    pub fn is_true(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("true"),
            PhpType::Nullable(inner) => inner.is_true(),
            _ => false,
        }
    }

    /// Whether this type is `false` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?false` (nullable wrapper).
    pub fn is_false(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("false"),
            PhpType::Nullable(inner) => inner.is_false(),
            _ => false,
        }
    }

    /// Whether this type is `int` or `integer` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?int` (nullable wrapper).
    pub fn is_int(&self) -> bool {
        match self {
            PhpType::Named(s) => matches!(s.to_ascii_lowercase().as_str(), "int" | "integer"),
            PhpType::Nullable(inner) => inner.is_int(),
            _ => false,
        }
    }

    /// Whether this type is `string` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?string` (nullable wrapper).
    pub fn is_string_type(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("string"),
            PhpType::Nullable(inner) => inner.is_string_type(),
            _ => false,
        }
    }

    /// Whether this type is `float` or `double` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?float` (nullable wrapper).
    pub fn is_float(&self) -> bool {
        match self {
            PhpType::Named(s) => matches!(s.to_ascii_lowercase().as_str(), "float" | "double"),
            PhpType::Nullable(inner) => inner.is_float(),
            _ => false,
        }
    }

    /// Whether this type is a literal string value (e.g. `'hello'`, `"world"`).
    pub fn is_string_literal(&self) -> bool {
        matches!(self, PhpType::Literal(LiteralValue::String(_)))
    }

    /// Whether this type is a literal integer value (e.g. `42`, `-1`).
    pub fn is_int_literal(&self) -> bool {
        matches!(self, PhpType::Literal(LiteralValue::Int(_)))
    }

    /// Whether this type is `string` or any PHPDoc string refinement (case-insensitive).
    ///
    /// Returns `true` for `string`, `non-empty-string`, `numeric-string`,
    /// `literal-string`, `truthy-string`, `callable-string`, `class-string`,
    /// `interface-string`, `lowercase-string`, `non-falsy-string`,
    /// `ClassString(…)`, `InterfaceString(…)`, and string literals.
    pub fn is_string_subtype(&self) -> bool {
        match self {
            PhpType::Named(s) => matches!(
                s.to_ascii_lowercase().as_str(),
                "string"
                    | "non-empty-string"
                    | "numeric-string"
                    | "literal-string"
                    | "truthy-string"
                    | "callable-string"
                    | "class-string"
                    | "interface-string"
                    | "lowercase-string"
                    | "non-falsy-string"
            ),
            PhpType::ClassString(_) | PhpType::InterfaceString(_) => true,
            PhpType::Literal(LiteralValue::String(_)) => true,
            PhpType::Nullable(inner) => inner.is_string_subtype(),
            PhpType::Generic(name, _) => matches!(
                name.to_ascii_lowercase().as_str(),
                "class-string" | "interface-string" | "model-property"
            ),
            PhpType::Union(members) => {
                !members.is_empty() && members.iter().all(|m| m.is_string_subtype())
            }
            _ => false,
        }
    }

    /// Whether this type is `int` or any PHPDoc integer refinement (case-insensitive).
    ///
    /// Returns `true` for `int`, `integer`, `positive-int`, `negative-int`,
    /// `non-negative-int`, `non-positive-int`, `non-zero-int`, `IntRange(…)`,
    /// and integer literals.
    pub fn is_int_subtype(&self) -> bool {
        match self {
            PhpType::Named(s) => matches!(
                s.to_ascii_lowercase().as_str(),
                "int"
                    | "integer"
                    | "positive-int"
                    | "negative-int"
                    | "non-negative-int"
                    | "non-positive-int"
                    | "non-zero-int"
            ),
            PhpType::IntRange(_, _) => true,
            PhpType::Literal(LiteralValue::Int(_)) => true,
            PhpType::Nullable(inner) => inner.is_int_subtype(),
            PhpType::Union(members) => {
                !members.is_empty() && members.iter().all(|m| m.is_int_subtype())
            }
            _ => false,
        }
    }

    /// Whether this type is `float`, `double`, a float literal, or a
    /// union of float subtypes (case-insensitive).
    ///
    /// Extends [`is_float`] with literal and union handling for
    /// symmetry with [`is_string_subtype`] and [`is_int_subtype`].
    pub fn is_float_subtype(&self) -> bool {
        match self {
            PhpType::Literal(LiteralValue::Float(_)) => true,
            PhpType::Union(members) => {
                !members.is_empty() && members.iter().all(|m| m.is_float_subtype())
            }
            _ => self.is_float(),
        }
    }

    /// Whether this type is `object` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?object` (nullable wrapper).
    pub fn is_object(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("object"),
            PhpType::Nullable(inner) => inner.is_object(),
            _ => false,
        }
    }

    /// Whether this type is `array-key` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?array-key` (nullable wrapper).
    pub fn is_array_key(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("array-key"),
            PhpType::Nullable(inner) => inner.is_array_key(),
            _ => false,
        }
    }

    /// Whether this type is `callable`, `Closure`, or a callable specification
    /// (case-insensitive).
    ///
    /// Also returns `true` when the type is `?callable` (nullable wrapper)
    /// or a `Callable { .. }` variant.
    pub fn is_callable(&self) -> bool {
        match self {
            PhpType::Named(s) => {
                let trimmed = s.strip_prefix('\\').unwrap_or(s);
                trimmed.eq_ignore_ascii_case("callable") || trimmed.eq_ignore_ascii_case("Closure")
            }
            PhpType::Callable { .. } => true,
            PhpType::Nullable(inner) => inner.is_callable(),
            _ => false,
        }
    }

    /// Whether this type is `iterable` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?iterable` (nullable wrapper).
    pub fn is_iterable(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("iterable"),
            PhpType::Nullable(inner) => inner.is_iterable(),
            _ => false,
        }
    }

    /// Whether this type is `Closure` (case-insensitive, with or without
    /// leading backslash).
    ///
    /// Also returns `true` when the type is `?Closure` (nullable wrapper)
    /// or a `Callable { kind, .. }` variant whose kind contains `"Closure"`.
    ///
    /// Unlike [`is_callable`], this does **not** match the bare `callable`
    /// keyword — only `Closure` and its callable-specification variants.
    pub fn is_closure(&self) -> bool {
        match self {
            PhpType::Named(s) => {
                let trimmed = s.strip_prefix('\\').unwrap_or(s);
                trimmed.eq_ignore_ascii_case("Closure")
            }
            PhpType::Callable { kind, .. } => kind.eq_ignore_ascii_case("Closure"),
            PhpType::Nullable(inner) => inner.is_closure(),
            _ => false,
        }
    }

    /// Whether this type is `resource` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?resource` (nullable wrapper).
    pub fn is_resource(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("resource"),
            PhpType::Nullable(inner) => inner.is_resource(),
            _ => false,
        }
    }

    /// Whether this type is a `Named` variant whose name equals `name`
    /// (case-sensitive comparison).
    ///
    /// Replaces the common `matches!(ty, PhpType::Named(n) if n == name)`
    /// pattern used for template parameter identity checks.
    pub fn is_named(&self, name: &str) -> bool {
        matches!(self, PhpType::Named(n) if n == name)
    }

    /// Whether this type is a `Named` variant whose name equals `name`
    /// (case-insensitive comparison).
    ///
    /// Replaces `matches!(ty, PhpType::Named(n) if n.eq_ignore_ascii_case(name))`
    /// patterns.
    pub fn is_named_ci(&self, name: &str) -> bool {
        matches!(self, PhpType::Named(n) if n.eq_ignore_ascii_case(name))
    }

    /// Returns `true` when this type is always coerced to `int` when
    /// used as an array key (int subtypes, float, bool, null).
    pub fn is_int_coercible_key(&self) -> bool {
        match self {
            PhpType::Named(s) => matches!(
                s.to_ascii_lowercase().as_str(),
                "int"
                    | "integer"
                    | "float"
                    | "double"
                    | "bool"
                    | "boolean"
                    | "true"
                    | "false"
                    | "null"
                    | "positive-int"
                    | "negative-int"
                    | "non-negative-int"
                    | "non-positive-int"
                    | "non-zero-int"
            ),
            _ => false,
        }
    }

    /// If this is a `Named` type that refers to a class (not a scalar,
    /// keyword, or pseudo-type), return its name.  Returns `None` for
    /// scalars (`int`, `string`, …), keywords (`mixed`, `void`, …),
    /// and non-`Named` variants.
    pub fn class_name(&self) -> Option<&str> {
        if let PhpType::Named(name) = self
            && is_class_like_name(name)
        {
            return Some(name.as_str());
        }
        None
    }

    /// Whether this type is a top-level `self`, `static`, or `$this`
    /// reference (case-insensitive) — the subset of self-like keywords
    /// that resolve to the *declaring* class, excluding `parent`.
    ///
    /// Unlike [`is_self_like`], this does **not** match `parent` and
    /// does **not** recurse into `Nullable` or `Union` wrappers.  It
    /// returns `true` only for a bare `PhpType::Named("self")` (and
    /// the other two variants).  Use this when you need to detect
    /// exactly the names that [`replace_self`] would rewrite, without
    /// unwrapping nullable/union layers.
    pub fn is_self_ref(&self) -> bool {
        matches!(self, PhpType::Named(s) if is_self_ref_name(s))
    }

    /// Whether this type is one of the self-referencing keywords:
    /// `self`, `static`, `$this`, or `parent` (case-insensitive).
    ///
    /// Also returns `true` when the type is nullable (e.g. `?static`).
    /// Returns `true` when this type refers to the `parent` keyword
    /// (bare, nullable, or in a union with null).
    pub fn is_parent_ref(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("parent"),
            PhpType::Generic(name, _) => name.eq_ignore_ascii_case("parent"),
            PhpType::Nullable(inner) => inner.is_parent_ref(),
            PhpType::Union(members) => {
                let non_null: Vec<_> = members.iter().filter(|m| !m.is_null()).collect();
                !non_null.is_empty() && non_null.iter().all(|m| m.is_parent_ref())
            }
            _ => false,
        }
    }

    pub fn is_self_like(&self) -> bool {
        match self {
            PhpType::Named(s) => self.is_self_ref() || s.eq_ignore_ascii_case("parent"),
            PhpType::Generic(name, _) => {
                // e.g. `self<RuleError>`, `static<T>` — check the generic base name directly.
                // Cannot use `base_name()` here because it filters out self-like
                // names via `is_scalar_name`.
                is_self_ref_name(name) || name.eq_ignore_ascii_case("parent")
            }
            PhpType::Nullable(inner) => inner.is_self_like(),
            PhpType::Union(members) => {
                // `static|null` — every non-null member is self-like.
                let non_null: Vec<_> = members.iter().filter(|m| !m.is_null()).collect();
                !non_null.is_empty() && non_null.iter().all(|m| m.is_self_like())
            }
            _ => false,
        }
    }

    /// Returns `true` when this type is exactly the bare, unparameterised
    /// `array` keyword — i.e. `PhpType::Named("array")`.
    ///
    /// Returns `false` for parameterised arrays (`array<int, string>`),
    /// array shapes (`array{key: string}`), slice syntax (`T[]`), `list`,
    /// `non-empty-array`, `iterable`, and any other array-like type.
    ///
    /// Use this when you need to distinguish a plain `array` return type
    /// (which carries no element-type information) from richer array types.
    pub fn is_bare_array(&self) -> bool {
        matches!(self, PhpType::Named(s) if s.eq_ignore_ascii_case("array"))
    }

    /// Returns `true` when this type represents an array-like PHP type.
    ///
    /// Matches:
    ///   - Named types: `array`, `list`, `non-empty-array`, `non-empty-list`, `iterable`
    ///   - Generic array types: `array<K, V>`, `list<T>`, `non-empty-array<K, V>`, etc.
    ///   - Array slice syntax: `T[]`
    ///   - Array shapes: `array{key: string, ...}`
    ///   - Nullable wrappers around any of the above
    pub fn is_array_like(&self) -> bool {
        match self {
            PhpType::Named(s) => is_array_like_name(s),
            PhpType::Generic(name, _) => is_array_like_name(name),
            PhpType::Array(_) => true,
            PhpType::ArrayShape(_) => true,
            PhpType::Nullable(inner) => inner.is_array_like(),
            _ => false,
        }
    }

    /// Returns true when this type represents an object (class instance, object keyword, or object shape).
    pub fn is_object_like(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("object") || !is_scalar_name(s),
            PhpType::Generic(name, _) => !is_scalar_name(name),
            PhpType::ObjectShape(_) => true,
            PhpType::Nullable(inner) => inner.is_object_like(),
            _ => false,
        }
    }

    /// Matches built-in PHP types and common PHPDoc pseudo-types like
    /// `mixed`, `class-string`, etc.
    pub fn is_scalar(&self) -> bool {
        match self {
            PhpType::Named(s) => is_scalar_name(s),
            PhpType::Nullable(inner) => inner.is_scalar(),
            PhpType::Generic(name, _) => is_scalar_name(name),
            PhpType::Array(_) => true,
            PhpType::ArrayShape(_) => true,
            PhpType::ObjectShape(_) => true,
            PhpType::Callable { .. } => true,
            PhpType::ClassString(_) => true,
            PhpType::InterfaceString(_) => true,
            PhpType::KeyOf(_) => true,
            PhpType::ValueOf(_) => true,
            PhpType::IntRange(_, _) => true,
            PhpType::Literal(_) => true,
            PhpType::Raw(_) => false,
            // Union, Intersection, Conditional, IndexAccess are
            // composite — not scalar by themselves.
            _ => false,
        }
    }

    /// Returns `true` when the type is scalar and carries no non-scalar
    /// generic arguments.  Unlike [`is_scalar`], `list<User>` returns
    /// `false` here because iterating it yields the non-scalar `User`.
    /// This is used by [`extract_value_type`] to decide whether to skip
    /// an element type: `array<int, list<Rule>>` should still yield
    /// `list<Rule>` even with `skip_scalar=true`.
    pub fn is_scalar_leaf(&self) -> bool {
        match self {
            PhpType::Generic(name, args) => {
                is_scalar_name(name) && args.iter().all(|a| a.is_scalar_leaf())
            }
            PhpType::Array(inner) => inner.is_scalar_leaf(),
            PhpType::Nullable(inner) => inner.is_scalar_leaf(),
            _ => self.is_scalar(),
        }
    }

    /// Extract the base class name from a type, if it refers to a single
    /// named class (possibly with generic parameters).
    ///
    /// Returns `Some("User")` for `User`, `Collection<int, User>`,
    /// `?User`, etc. Returns `None` for unions, intersections, scalars,
    /// callables, shapes, and other non-class types.
    pub fn base_name(&self) -> Option<&str> {
        match self {
            PhpType::Named(s) if !is_scalar_name(s) => {
                Some(s.strip_prefix('\\').unwrap_or(s.as_str()))
            }
            PhpType::Generic(name, _) if !is_scalar_name(name) => {
                Some(name.strip_prefix('\\').unwrap_or(name.as_str()))
            }
            PhpType::Nullable(inner) => inner.base_name(),
            _ => None,
        }
    }

    /// Convert this type to a valid native PHP type hint string.
    ///
    /// Returns `None` when the type has no native representation (e.g.
    /// `array{key: string}`, `callable(int): void`, conditional types).
    ///
    /// Rich PHPStan types are simplified to their native equivalents:
    /// - `list<T>`, `non-empty-list<T>`, `non-empty-array<K,V>`,
    ///   `array<K,V>`, `associative-array<K,V>` → `array`
    /// - `Collection<T>` (any generic class) → `Collection`
    /// - `positive-int`, `negative-int`, `non-negative-int`,
    ///   `non-positive-int`, `non-zero-int` → `int`
    /// - `non-empty-string`, `numeric-string`, `class-string`,
    ///   `literal-string`, etc. → `string`
    /// - `scalar`, `numeric`, `number` → no native equivalent (`None`)
    /// - `array-key` → no native equivalent (`None`)
    /// - Unions/intersections of native types are preserved
    /// - `?T` → `?NativeT`
    pub fn to_native_hint(&self) -> Option<String> {
        match self {
            PhpType::Named(s) => native_scalar_name(s).map(|n| n.to_string()),
            PhpType::Generic(name, _) => {
                // Generic classes: strip the generic params.
                // `array<K,V>` → `array`, `Collection<T>` → `Collection`
                native_scalar_name(name)
                    .map(|n| n.to_string())
                    .or_else(|| Some(name.clone()))
            }
            PhpType::Nullable(inner) => inner.to_native_hint().map(|n| format!("?{}", n)),
            PhpType::Union(members) => {
                let native: Vec<String> =
                    members.iter().filter_map(|m| m.to_native_hint()).collect();
                if native.len() != members.len() {
                    return None; // some members have no native form
                }
                // Deduplicate (e.g. `list<string>|array<int>` both → `array`)
                let mut deduped = native;
                deduped.sort();
                deduped.dedup();
                Some(deduped.join("|"))
            }
            PhpType::Intersection(members) => {
                let native: Vec<String> =
                    members.iter().filter_map(|m| m.to_native_hint()).collect();
                if native.len() != members.len() {
                    return None;
                }
                Some(native.join("&"))
            }
            PhpType::Array(_) | PhpType::ArrayShape(_) => Some("array".to_string()),
            PhpType::ClassString(_)
            | PhpType::InterfaceString(_)
            | PhpType::Literal(LiteralValue::String(_)) => Some("string".to_string()),
            PhpType::IntRange(_, _) | PhpType::Literal(LiteralValue::Int(_)) => {
                Some("int".to_string())
            }
            PhpType::Literal(LiteralValue::Float(_)) => Some("float".to_string()),
            PhpType::ObjectShape(_) => Some("object".to_string()),
            PhpType::Callable { kind, .. } => Some(kind.clone()),
            // Conditionals, key-of, value-of, index-access, and raw
            // types have no native form.
            PhpType::Conditional { .. }
            | PhpType::KeyOf(_)
            | PhpType::ValueOf(_)
            | PhpType::IndexAccess(_, _)
            | PhpType::Raw(_) => None,
        }
    }

    /// Like [`to_native_hint`] but returns a structured [`PhpType`] instead of a string,
    /// avoiding a parse round-trip.
    pub fn to_native_hint_typed(&self) -> Option<PhpType> {
        match self {
            PhpType::Named(s) => native_scalar_name(s).map(|n| PhpType::Named(n.to_string())),
            PhpType::Generic(name, _) => {
                // Generic classes: strip the generic params.
                // `array<K,V>` → `array`, `Collection<T>` → `Collection`
                native_scalar_name(name)
                    .map(|n| PhpType::Named(n.to_string()))
                    .or_else(|| Some(PhpType::Named(name.clone())))
            }
            PhpType::Nullable(inner) => inner
                .to_native_hint_typed()
                .map(|n| PhpType::Nullable(Box::new(n))),
            PhpType::Union(members) => {
                let native: Vec<PhpType> = members
                    .iter()
                    .filter_map(|m| m.to_native_hint_typed())
                    .collect();
                if native.len() != members.len() {
                    return None; // some members have no native form
                }
                // Deduplicate (e.g. `list<string>|array<int>` both → `array`)
                let mut deduped = Vec::new();
                for ty in native {
                    if !deduped
                        .iter()
                        .any(|existing: &PhpType| existing.equivalent(&ty))
                    {
                        deduped.push(ty);
                    }
                }
                if deduped.len() == 1 {
                    Some(deduped.into_iter().next().unwrap())
                } else {
                    Some(PhpType::Union(deduped))
                }
            }
            PhpType::Intersection(members) => {
                let native: Vec<PhpType> = members
                    .iter()
                    .filter_map(|m| m.to_native_hint_typed())
                    .collect();
                if native.len() != members.len() {
                    return None;
                }
                // Deduplicate
                let mut deduped = Vec::new();
                for ty in native {
                    if !deduped
                        .iter()
                        .any(|existing: &PhpType| existing.equivalent(&ty))
                    {
                        deduped.push(ty);
                    }
                }
                if deduped.len() == 1 {
                    Some(deduped.into_iter().next().unwrap())
                } else {
                    Some(PhpType::Intersection(deduped))
                }
            }
            PhpType::Array(_) | PhpType::ArrayShape(_) => Some(PhpType::array()),
            PhpType::ClassString(_)
            | PhpType::InterfaceString(_)
            | PhpType::Literal(LiteralValue::String(_)) => Some(PhpType::string()),
            PhpType::IntRange(_, _) | PhpType::Literal(LiteralValue::Int(_)) => {
                Some(PhpType::int())
            }
            PhpType::Literal(LiteralValue::Float(_)) => Some(PhpType::float()),
            PhpType::ObjectShape(_) => Some(PhpType::object()),
            PhpType::Callable { kind, .. } => Some(PhpType::Named(kind.clone())),
            PhpType::Conditional { .. }
            | PhpType::KeyOf(_)
            | PhpType::ValueOf(_)
            | PhpType::IndexAccess(_, _)
            | PhpType::Raw(_) => None,
        }
    }

    /// Return the top-level union members if this is a union type,
    /// or a single-element slice containing `self` otherwise.
    ///
    /// This replaces `split_top_level_union` for structured types.
    pub fn union_members(&self) -> Vec<&PhpType> {
        match self {
            PhpType::Union(members) => members.iter().collect(),
            other => vec![other],
        }
    }

    /// Return the top-level intersection members if this is an intersection
    /// type, or a single-element slice containing `self` otherwise.
    pub fn intersection_members(&self) -> Vec<&PhpType> {
        match self {
            PhpType::Intersection(members) => members.iter().collect(),
            other => vec![other],
        }
    }

    /// Extract the "value" type from a generic iterable type.
    ///
    /// Returns the element type that iteration would yield as a value:
    ///   - `User[]`                        → `Some(Named("User"))`
    ///   - `list<User>`                    → `Some(Named("User"))`
    ///   - `array<int, User>`              → `Some(Named("User"))`
    ///   - `Collection<int, User>`         → `Some(Named("User"))`
    ///   - `Generator<int, User, …>`       → `Some(Named("User"))` (2nd param)
    ///   - `?list<User>`                   → `Some(Named("User"))`
    ///   - `int`                           → `None`
    ///
    /// When `skip_scalar` is true, returns `None` if the extracted type
    /// is a scalar (for class-based completion). When false, returns any
    /// element type (matching `extract_iterable_element_type` behaviour).
    pub fn extract_value_type(&self, skip_scalar: bool) -> Option<&PhpType> {
        match self {
            PhpType::Array(inner) => {
                if skip_scalar && inner.is_scalar() {
                    None
                } else {
                    Some(inner.as_ref())
                }
            }
            PhpType::Generic(name, args) if !args.is_empty() => {
                let value = if Self::short_name_of(name) == "Generator" {
                    // Generator<TKey, TValue, TSend, TReturn>: value is
                    // the 2nd param (index 1). When only one param is
                    // given, treat it as the value type.
                    args.get(1).or(args.last())
                } else {
                    // Default: last generic parameter (works for array,
                    // list, iterable, Collection, etc.).
                    args.last()
                };
                match value {
                    Some(v) if skip_scalar && v.is_scalar_leaf() => None,
                    Some(v) => Some(v),
                    None => None,
                }
            }
            PhpType::Nullable(inner) => inner.extract_value_type(skip_scalar),
            PhpType::Union(members) => members
                .iter()
                .find_map(|m| m.extract_value_type(skip_scalar)),
            _ => None,
        }
    }

    /// Extract the "key" type from a generic iterable type.
    ///
    /// Returns the key type only when the generic has 2+ parameters:
    ///   - `array<string, User>`  → `Some(Named("string"))`
    ///   - `array<int, User>`     → `Some(Named("int"))`
    ///   - `list<User>`           → `None` (single param → implicit int key)
    ///   - `User[]`               → `None` (shorthand → implicit int key)
    ///
    /// When `skip_scalar` is true, returns `None` if the key type is
    /// scalar.
    pub fn extract_key_type(&self, skip_scalar: bool) -> Option<&PhpType> {
        match self {
            PhpType::Generic(_, args) if args.len() >= 2 => {
                let key = &args[0];
                if skip_scalar && key.is_scalar() {
                    None
                } else {
                    Some(key)
                }
            }
            PhpType::Nullable(inner) => inner.extract_key_type(skip_scalar),
            PhpType::Union(members) => members.iter().find_map(|m| m.extract_key_type(skip_scalar)),
            _ => None,
        }
    }

    /// Extract the element (value) type from an iterable, including
    /// scalar element types.
    ///
    /// This is the `PhpType` equivalent of `extract_iterable_element_type`.
    /// Unlike `extract_value_type(true)`, this never skips scalars.
    pub fn extract_element_type(&self) -> Option<&PhpType> {
        self.extract_value_type(false)
    }

    /// Look up the value type for a specific key in an array shape.
    ///
    /// Given a parsed `array{name: string, user: User}` and key `"user"`,
    /// returns `Some(&PhpType::Named("User"))`.
    ///
    /// For positional (unkeyed) entries like `array{User, Address}`, a
    /// numeric string key (e.g. `"0"`, `"1"`) matches the entry at that
    /// index position. This mirrors PHPStan's behaviour where positional
    /// entries implicitly have numeric keys.
    ///
    /// Also handles nullable shapes (`?array{…}`) by delegating to the
    /// inner type.
    ///
    /// Returns `None` if this is not an array shape or the key is not found.
    pub fn shape_value_type(&self, key: &str) -> Option<&PhpType> {
        match self {
            PhpType::ArrayShape(entries) => {
                // First try an exact key match (handles named and explicit
                // numeric keys like `array{0: User, 1: Address}`).
                if let Some(entry) = entries.iter().find(|e| e.key.as_deref() == Some(key)) {
                    return Some(&entry.value_type);
                }
                // Fall back to positional index matching: if the key is a
                // valid numeric index, match the Nth positional (unkeyed)
                // entry. This handles `array{User, Address}` where the
                // entries have `key: None`.
                if let Ok(idx) = key.parse::<usize>() {
                    let mut positional_idx = 0usize;
                    for entry in entries {
                        if entry.key.is_none() {
                            if positional_idx == idx {
                                return Some(&entry.value_type);
                            }
                            positional_idx += 1;
                        }
                    }
                }
                None
            }
            PhpType::Nullable(inner) => inner.shape_value_type(key),
            PhpType::Union(members) => members.iter().find_map(|m| m.shape_value_type(key)),
            _ => None,
        }
    }

    /// Look up the value type for a specific key in an array shape,
    /// returning an owned `PhpType`.
    ///
    /// Unlike [`shape_value_type`](Self::shape_value_type), this method
    /// accounts for optional entries: when a key is marked optional
    /// (`key?: type`), the returned type is wrapped in `Nullable` so
    /// that downstream narrowing can strip `null` when the key is
    /// known to be present.
    ///
    /// Returns `None` if this is not an array shape or the key is not
    /// found.
    pub fn extract_shape_key_type(&self, key: &str) -> Option<PhpType> {
        match self {
            PhpType::ArrayShape(entries) => {
                if let Some(entry) = entries.iter().find(|e| e.key.as_deref() == Some(key)) {
                    return if entry.optional {
                        Some(PhpType::Nullable(Box::new(entry.value_type.clone())))
                    } else {
                        Some(entry.value_type.clone())
                    };
                }
                if let Ok(idx) = key.parse::<usize>() {
                    let mut positional_idx = 0usize;
                    for entry in entries {
                        if entry.key.is_none() {
                            if positional_idx == idx {
                                return if entry.optional {
                                    Some(PhpType::Nullable(Box::new(entry.value_type.clone())))
                                } else {
                                    Some(entry.value_type.clone())
                                };
                            }
                            positional_idx += 1;
                        }
                    }
                }
                None
            }
            PhpType::Nullable(inner) => inner.extract_shape_key_type(key),
            PhpType::Union(members) => members.iter().find_map(|m| m.extract_shape_key_type(key)),
            _ => None,
        }
    }

    /// Return the shape entries if this is an `ArrayShape` or `ObjectShape`.
    ///
    /// Also handles nullable shapes by delegating to the inner type.
    /// Returns `None` for all other variants.
    pub fn shape_entries(&self) -> Option<&[ShapeEntry]> {
        match self {
            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => Some(entries),
            PhpType::Nullable(inner) => inner.shape_entries(),
            PhpType::Union(members) => {
                // Find the first array/object shape member in the union.
                members.iter().find_map(|m| m.shape_entries())
            }
            _ => None,
        }
    }

    /// Return `true` if this type is an array shape (`array{…}`).
    ///
    /// Also returns `true` for `?array{…}`.
    pub fn is_array_shape(&self) -> bool {
        match self {
            PhpType::ArrayShape(_) => true,
            PhpType::Nullable(inner) => inner.is_array_shape(),
            _ => false,
        }
    }

    /// Return `true` if this type is an object shape (`object{…}`).
    ///
    /// Also returns `true` for `?object{…}`.
    pub fn is_object_shape(&self) -> bool {
        match self {
            PhpType::ObjectShape(_) => true,
            PhpType::Nullable(inner) => inner.is_object_shape(),
            _ => false,
        }
    }

    /// Look up the value type for a specific property in an object shape.
    ///
    /// Given a parsed `object{name: string, user: User}` and key `"user"`,
    /// returns `Some(&PhpType::Named("User"))`.
    ///
    /// Also handles nullable object shapes (`?object{…}`).
    ///
    /// Returns `None` if this is not an object shape or the property
    /// is not found.
    pub fn object_shape_property_type(&self, prop: &str) -> Option<&PhpType> {
        match self {
            PhpType::ObjectShape(entries) => entries
                .iter()
                .find(|e| e.key.as_deref() == Some(prop))
                .map(|e| &e.value_type),
            PhpType::Nullable(inner) => inner.object_shape_property_type(prop),
            _ => None,
        }
    }

    /// Extract parameter types from a `Callable` variant.
    ///
    /// Returns the parameter list for callable/Closure types without
    /// round-tripping through string serialization.
    ///
    ///   - `callable(int, string): bool` → `Some(&[CallableParam { .. }, ..])`
    ///   - `?Closure(int): void`         → `Some(&[CallableParam { .. }])`
    ///   - `Closure(int)|null`           → `Some(&[CallableParam { .. }])`
    ///   - `int`                         → `None`
    pub fn callable_param_types(&self) -> Option<&[CallableParam]> {
        match self {
            PhpType::Callable { params, .. } => Some(params.as_slice()),
            PhpType::Nullable(inner) => inner.callable_param_types(),
            PhpType::Union(members) => {
                for member in members {
                    if let Some(params) = member.callable_param_types() {
                        return Some(params);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Extract the return type from a `Callable` variant.
    ///
    /// Returns the return type for callable/Closure types without
    /// round-tripping through string serialization.
    ///
    ///   - `callable(int): User`  → `Some(Named("User"))`
    ///   - `Closure(): void`      → `Some(Named("void"))`
    ///   - `?Closure(): User`     → `Some(Named("User"))`
    ///   - `callable`             → `None` (no return type specified)
    ///   - `int`                  → `None`
    pub fn callable_return_type(&self) -> Option<&PhpType> {
        match self {
            PhpType::Callable { return_type, .. } => return_type.as_deref(),
            PhpType::Nullable(inner) => inner.callable_return_type(),
            PhpType::Union(members) => {
                for member in members {
                    if let Some(ret) = member.callable_return_type() {
                        return Some(ret);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Extract the TSend type (3rd generic parameter) from a Generator.
    ///
    /// `Generator<TKey, TValue, TSend, TReturn>` — the send type is the
    /// 3rd parameter (index 2).
    ///
    ///   - `Generator<int, string, MyClass, void>` → `Some(Named("MyClass"))`
    ///   - `?Generator<int, string, MyClass, void>` → `Some(Named("MyClass"))`
    ///   - `Generator<int, string>`                 → `None` (fewer than 3 params)
    ///   - `int`                                    → `None`
    ///
    /// When `skip_scalar` is true, returns `None` if the send type is
    /// scalar (matching the pattern used by `extract_value_type`).
    pub fn generator_send_type(&self, skip_scalar: bool) -> Option<&PhpType> {
        match self {
            PhpType::Generic(name, args) if Self::short_name_of(name) == "Generator" => {
                match args.get(2) {
                    Some(send) if skip_scalar && send.is_scalar() => None,
                    Some(send) => Some(send),
                    None => None,
                }
            }
            PhpType::Nullable(inner) => inner.generator_send_type(skip_scalar),
            _ => None,
        }
    }

    /// Return the non-null part of a type.
    ///
    /// For a union like `User|null`, returns `Some(Named("User"))`.
    /// For `User|Admin|null`, returns `Some(Union([Named("User"), Named("Admin")]))`.
    /// For a type that doesn't contain `null`, returns `None`.
    /// For bare `null`, returns `None`.
    ///
    /// This extracts the non-null part from a union type.
    pub fn non_null_type(&self) -> Option<PhpType> {
        match self {
            PhpType::Nullable(inner) => Some(inner.as_ref().clone()),
            PhpType::Union(members) => {
                let non_null: Vec<&PhpType> = members.iter().filter(|m| !m.is_null()).collect();
                match non_null.len() {
                    0 => None,
                    1 => Some(non_null[0].clone()),
                    _ => Some(PhpType::Union(non_null.into_iter().cloned().collect())),
                }
            }
            // Not a union or nullable — no null to strip.
            _ => None,
        }
    }

    /// Unwrap one layer of `Nullable`, returning the inner type.
    ///
    /// For `Nullable(inner)` returns `inner`, for everything else returns `self`.
    /// This is a cheap, borrowing alternative to [`non_null_type`] which
    /// returns an owned `PhpType` and also handles union-with-null.
    pub fn unwrap_nullable(&self) -> &PhpType {
        match self {
            PhpType::Nullable(inner) => inner.as_ref(),
            _ => self,
        }
    }

    /// Whether every atomic member of `self` also appears in `other`.
    ///
    /// Used to detect when a forward-walker narrowing result is a strict
    /// subset of the AST-based parameter type (e.g. `null` ⊆ `string|null`,
    /// `Foo` ⊆ `Foo|Bar|null`).  Only checks shallow structural equality
    /// of union/nullable members — does not consider class hierarchy.
    pub fn is_subset_of(&self, other: &PhpType) -> bool {
        // `mixed` is the top type — everything is a subset of it.
        if other.is_mixed() && !self.is_mixed() {
            return true;
        }
        let self_members = self.atomic_members();
        let other_members = other.atomic_members();
        if self_members.is_empty() {
            return false;
        }
        self_members
            .iter()
            .all(|s| other_members.iter().any(|o| s.equivalent(o)))
    }

    /// Collect the atomic (leaf) type members of a type.
    ///
    /// `Foo|Bar|null` → `[Foo, Bar, null]`, `?Foo` → `[Foo, null]`,
    /// `Foo` → `[Foo]`.
    fn atomic_members(&self) -> Vec<PhpType> {
        match self {
            PhpType::Union(members) => members.clone(),
            PhpType::Nullable(inner) => {
                vec![inner.as_ref().clone(), PhpType::null()]
            }
            _ => vec![self.clone()],
        }
    }

    /// Whether all non-null members of this type are scalar.
    ///
    /// For unions like `string|null`, returns `true`.
    /// For `User|null`, returns `false` (User is a class).
    /// For bare scalars like `int`, returns `true`.
    /// For bare classes like `User`, returns `false`.
    ///
    /// Checks whether a type is purely scalar.
    pub fn all_members_scalar(&self) -> bool {
        match self {
            PhpType::Union(members) => members
                .iter()
                .filter(|m| !m.is_null())
                .all(|m| m.is_scalar()),
            PhpType::Nullable(inner) => inner.is_scalar(),
            other => other.is_scalar(),
        }
    }

    /// If this is a `class-string<T>`, returns `Some(&T)`. Otherwise, returns `None`.
    pub fn unwrap_class_string_inner(&self) -> Option<&PhpType> {
        match self {
            PhpType::ClassString(Some(inner)) => Some(inner.as_ref()),
            _ => None,
        }
    }

    /// Like [`all_members_scalar`] but uses the narrow
    /// [`is_primitive_scalar`] check.
    ///
    /// Returns `true` only when every non-null member of the type is a
    /// primitive scalar (int, string, bool, float, array, void, never,
    /// etc.).  Returns `false` for `mixed`, `object`, `class-string`,
    /// and other pseudo-types on which member access may be valid.
    ///
    /// Checks whether all members are primitive scalar types.
    pub fn all_members_primitive_scalar(&self) -> bool {
        match self {
            PhpType::Union(members) => members
                .iter()
                .filter(|m| !m.is_null())
                .all(|m| m.is_primitive_scalar()),
            PhpType::Nullable(inner) => inner.is_primitive_scalar(),
            other => other.is_primitive_scalar(),
        }
    }

    /// Produce a new `PhpType` with `self`, `static`, and `$this`
    /// replaced by the given class name.
    ///
    /// Walks the entire type tree and replaces any `Named("self")`,
    /// `Named("static")`, or `Named("$this")` with
    /// `Named(class_name)`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let ty = PhpType::parse("self|null");
    /// let replaced = ty.replace_self("App\\User");
    /// assert_eq!(replaced.to_string(), "App\\User | null");
    /// ```
    pub fn replace_self(&self, class_name: &str) -> PhpType {
        self.replace_self_with_type(&PhpType::Named(class_name.to_string()))
    }

    /// Replace only the `self` keyword (not `static` or `$this`) with a
    /// concrete class name.  Used during inheritance merging so that
    /// inherited methods carry the declaring class's identity for `self`
    /// while preserving `static` for late-static-binding resolution.
    pub fn replace_bare_self(&self, class_name: &str) -> PhpType {
        match self {
            PhpType::Named(s) if s.eq_ignore_ascii_case("self") => {
                PhpType::Named(class_name.to_string())
            }
            PhpType::Named(_) | PhpType::Literal(_) | PhpType::Raw(_) => self.clone(),
            PhpType::Nullable(inner) => {
                PhpType::Nullable(Box::new(inner.replace_bare_self(class_name)))
            }
            PhpType::Union(types) => PhpType::Union(
                types
                    .iter()
                    .map(|t| t.replace_bare_self(class_name))
                    .collect(),
            ),
            PhpType::Intersection(types) => PhpType::Intersection(
                types
                    .iter()
                    .map(|t| t.replace_bare_self(class_name))
                    .collect(),
            ),
            PhpType::Generic(name, args) => {
                let resolved_name = if name.eq_ignore_ascii_case("self") {
                    class_name.to_string()
                } else {
                    name.clone()
                };
                PhpType::Generic(
                    resolved_name,
                    args.iter()
                        .map(|a| a.replace_bare_self(class_name))
                        .collect(),
                )
            }
            PhpType::Array(inner) => PhpType::Array(Box::new(inner.replace_bare_self(class_name))),
            _ => self.clone(),
        }
    }

    /// Returns `true` when this type contains the bare `self` keyword
    /// (not `static` or `$this`).
    pub fn contains_bare_self(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("self"),
            PhpType::Nullable(inner) => inner.contains_bare_self(),
            PhpType::Union(types) | PhpType::Intersection(types) => {
                types.iter().any(|t| t.contains_bare_self())
            }
            PhpType::Generic(name, args) => {
                name.eq_ignore_ascii_case("self") || args.iter().any(|a| a.contains_bare_self())
            }
            PhpType::Array(inner) => inner.contains_bare_self(),
            _ => false,
        }
    }

    /// Check whether this type tree contains any `self`, `static`, or
    /// `$this` references that [`replace_self`] / [`replace_self_with_type`]
    /// would replace.
    pub fn contains_self_ref(&self) -> bool {
        match self {
            PhpType::Named(_) => self.is_self_ref(),
            PhpType::Nullable(inner) => inner.contains_self_ref(),
            PhpType::Union(types) | PhpType::Intersection(types) => {
                types.iter().any(|t| t.contains_self_ref())
            }
            PhpType::Generic(name, args) => {
                is_self_ref_name(name) || args.iter().any(|a| a.contains_self_ref())
            }
            PhpType::Array(inner) => inner.contains_self_ref(),
            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
                entries.iter().any(|e| e.value_type.contains_self_ref())
            }
            PhpType::Callable {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| p.type_hint.contains_self_ref())
                    || return_type.as_ref().is_some_and(|r| r.contains_self_ref())
            }
            PhpType::Conditional {
                condition,
                then_type,
                else_type,
                ..
            } => {
                condition.contains_self_ref()
                    || then_type.contains_self_ref()
                    || else_type.contains_self_ref()
            }
            PhpType::ClassString(inner) | PhpType::InterfaceString(inner) => {
                inner.as_ref().is_some_and(|t| t.contains_self_ref())
            }
            PhpType::KeyOf(inner) | PhpType::ValueOf(inner) => inner.contains_self_ref(),
            PhpType::IndexAccess(base, index) => {
                base.contains_self_ref() || index.contains_self_ref()
            }
            PhpType::Literal(_) | PhpType::Raw(_) | PhpType::IntRange(_, _) => false,
        }
    }

    /// Replace `self` / `static` / `$this` throughout this type tree
    /// with the given [`PhpType`].
    ///
    /// This is the structured counterpart of [`replace_self`]: instead of
    /// replacing with a bare class name (`PhpType::Named(name)`), it
    /// substitutes a full type expression.  This preserves generic
    /// parameters when the receiver is a generic type like
    /// `Builder<Article>`.
    ///
    /// When `replacement` is `PhpType::Generic("Builder", [Named("Article")])`
    /// and the return type is `Named("static")`, the result is the full
    /// generic type.  When the return type is `Generic("static", [args])`,
    /// the replacement's base name is used and the return type's own args
    /// are kept (they override the receiver's args).
    pub fn replace_self_with_type(&self, replacement: &PhpType) -> PhpType {
        // Extract the base class name from the replacement for use in
        // Generic nodes where only the name part is replaced.
        let replacement_name = match replacement {
            PhpType::Named(n) => n.as_str(),
            PhpType::Generic(n, _) => n.as_str(),
            _ => "",
        };
        match self {
            PhpType::Named(_) if self.is_self_ref() => replacement.clone(),

            PhpType::Named(_) | PhpType::Literal(_) | PhpType::Raw(_) => self.clone(),

            PhpType::Nullable(inner) => {
                PhpType::Nullable(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::Union(types) => PhpType::Union(
                types
                    .iter()
                    .map(|t| t.replace_self_with_type(replacement))
                    .collect(),
            ),

            PhpType::Intersection(types) => PhpType::Intersection(
                types
                    .iter()
                    .map(|t| t.replace_self_with_type(replacement))
                    .collect(),
            ),

            PhpType::Generic(name, args) => {
                let resolved_name = if is_self_ref_name(name) {
                    replacement_name.to_string()
                } else {
                    name.clone()
                };
                PhpType::Generic(
                    resolved_name,
                    args.iter()
                        .map(|a| a.replace_self_with_type(replacement))
                        .collect(),
                )
            }

            PhpType::Array(inner) => {
                PhpType::Array(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.replace_self_with_type(replacement),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.replace_self_with_type(replacement),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: kind.clone(),
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.replace_self_with_type(replacement),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type
                    .as_ref()
                    .map(|r| Box::new(r.replace_self_with_type(replacement))),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.replace_self_with_type(replacement)),
                then_type: Box::new(then_type.replace_self_with_type(replacement)),
                else_type: Box::new(else_type.replace_self_with_type(replacement)),
            },

            PhpType::ClassString(inner) => PhpType::ClassString(
                inner
                    .as_ref()
                    .map(|t| Box::new(t.replace_self_with_type(replacement))),
            ),

            PhpType::InterfaceString(inner) => PhpType::InterfaceString(
                inner
                    .as_ref()
                    .map(|t| Box::new(t.replace_self_with_type(replacement))),
            ),

            PhpType::KeyOf(inner) => {
                PhpType::KeyOf(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::ValueOf(inner) => {
                PhpType::ValueOf(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::IntRange(lo, hi) => PhpType::IntRange(lo.clone(), hi.clone()),

            PhpType::IndexAccess(base, index) => PhpType::IndexAccess(
                Box::new(base.replace_self_with_type(replacement)),
                Box::new(index.replace_self_with_type(replacement)),
            ),
        }
    }

    /// Substitute template parameter names throughout this type tree.
    ///
    /// Walks the entire type tree and replaces any `Named(s)` node whose
    /// name appears as a key in `subs` with `PhpType::parse(replacement)`.
    /// All other nodes are recursively rebuilt with their children
    /// substituted.
    ///
    /// This is the structured-type equivalent of the string-surgery
    /// `apply_substitution` function in `inheritance.rs`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::collections::HashMap;
    /// let ty = PhpType::parse("Collection<TKey, TValue>");
    /// let subs: HashMap<String, PhpType> =
    ///     [("TKey".into(), PhpType::parse("int")), ("TValue".into(), PhpType::parse("User"))]
    ///         .into_iter().collect();
    /// let result = ty.substitute(&subs);
    /// assert_eq!(result.to_string(), "Collection<int, User>");
    /// ```
    pub fn substitute(&self, subs: &std::collections::HashMap<String, PhpType>) -> PhpType {
        if subs.is_empty() {
            return self.clone();
        }
        match self {
            PhpType::Named(s) => {
                if let Some(replacement) = subs.get(s.as_str()) {
                    replacement.clone()
                } else {
                    self.clone()
                }
            }

            PhpType::Literal(_) | PhpType::Raw(_) | PhpType::IntRange(_, _) => self.clone(),

            PhpType::Nullable(inner) => {
                let resolved = inner.substitute(subs);
                // If the substitution produced a union or nullable,
                // don't double-wrap.
                match &resolved {
                    PhpType::Nullable(_) => resolved,
                    PhpType::Union(members) => {
                        // Already nullable if it contains null
                        if members.iter().any(
                            |m| matches!(m, PhpType::Named(n) if n.eq_ignore_ascii_case("null")),
                        ) {
                            resolved
                        } else {
                            PhpType::Nullable(Box::new(resolved))
                        }
                    }
                    _ => PhpType::Nullable(Box::new(resolved)),
                }
            }

            PhpType::Union(types) => {
                let resolved: Vec<PhpType> = types.iter().map(|t| t.substitute(subs)).collect();
                // Flatten any nested unions produced by substitution.
                let mut flat = Vec::with_capacity(resolved.len());
                for t in resolved {
                    match t {
                        PhpType::Union(inner) => flat.extend(inner),
                        other => flat.push(other),
                    }
                }
                if flat.len() == 1 {
                    flat.into_iter().next().unwrap()
                } else {
                    PhpType::Union(flat)
                }
            }

            PhpType::Intersection(types) => {
                let resolved: Vec<PhpType> = types.iter().map(|t| t.substitute(subs)).collect();
                let mut flat = Vec::with_capacity(resolved.len());
                for t in resolved {
                    match t {
                        PhpType::Intersection(inner) => flat.extend(inner),
                        other => flat.push(other),
                    }
                }
                if flat.len() == 1 {
                    flat.into_iter().next().unwrap()
                } else {
                    PhpType::Intersection(flat)
                }
            }

            PhpType::Generic(name, args) => {
                // The base name might itself be a template parameter.
                if let Some(replacement) = subs.get(name.as_str()) {
                    match replacement {
                        PhpType::Named(n) => PhpType::Generic(
                            n.clone(),
                            args.iter().map(|a| a.substitute(subs)).collect(),
                        ),
                        PhpType::Generic(base, _) => {
                            // Use the replacement's base name but keep the
                            // original generic args (substituted).  The
                            // replacement's own args are discarded because
                            // the source type provides its own parameters.
                            PhpType::Generic(
                                base.clone(),
                                args.iter().map(|a| a.substitute(subs)).collect(),
                            )
                        }
                        // For non-class replacements (union, intersection,
                        // etc.), the generic wrapper is meaningless — return
                        // the replacement as-is.
                        _ => replacement.clone(),
                    }
                } else {
                    PhpType::Generic(
                        name.clone(),
                        args.iter().map(|a| a.substitute(subs)).collect(),
                    )
                }
            }

            PhpType::Array(inner) => PhpType::Array(Box::new(inner.substitute(subs))),

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.substitute(subs),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.substitute(subs),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: kind.clone(),
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.substitute(subs),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type.as_ref().map(|r| Box::new(r.substitute(subs))),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.substitute(subs)),
                then_type: Box::new(then_type.substitute(subs)),
                else_type: Box::new(else_type.substitute(subs)),
            },

            PhpType::ClassString(inner) => {
                PhpType::ClassString(inner.as_ref().map(|t| Box::new(t.substitute(subs))))
            }

            PhpType::InterfaceString(inner) => {
                PhpType::InterfaceString(inner.as_ref().map(|t| Box::new(t.substitute(subs))))
            }

            PhpType::KeyOf(inner) => {
                let resolved = inner.substitute(subs);
                evaluate_key_of(&resolved)
            }

            PhpType::ValueOf(inner) => {
                let resolved = inner.substitute(subs);
                evaluate_value_of(&resolved)
            }

            PhpType::IndexAccess(base, index) => {
                let resolved_base = base.substitute(subs);
                let resolved_index = index.substitute(subs);
                evaluate_index_access(&resolved_base, &resolved_index)
            }
        }
    }

    /// Extract all class-like names from this type, recursively.
    ///
    /// Walks the entire type tree and collects the base names of all
    /// class-like types (including those nested inside generics,
    /// callables, shapes, etc.). Scalar types, keywords, `null`,
    /// and literals are skipped.
    ///
    /// For `Collection<int, User>|null`, returns `["Collection", "User"]`.
    /// For `?User`, returns `["User"]`.
    /// For `int|string`, returns `[]`.
    pub fn extract_class_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_class_names(&mut names);
        names
    }

    /// Extract only top-level class names from this type.
    ///
    /// Unlike [`extract_class_names`], this does **not** recurse into
    /// generic type arguments, callable parameters, shape entries, or
    /// other nested positions. It returns only the outermost class
    /// names that are directly part of the type expression.
    ///
    /// For `Collection<int, User>|null`, returns `["Collection"]`.
    /// For `User|Admin`, returns `["User", "Admin"]`.
    /// For `?User`, returns `["User"]`.
    /// For `User[]`, returns `["User"]`.
    /// For `int|string`, returns `[]`.
    ///
    /// This is the correct replacement for the string-based
    /// `extract_class_names_from_type_string` in
    /// `definition/type_definition.rs`, where go-to-type-definition
    /// should jump to the container class, not its type arguments.
    pub fn top_level_class_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_top_level_class_names(&mut names);
        names
    }

    /// Recursive helper for [`extract_class_names`].
    fn collect_class_names(&self, names: &mut Vec<String>) {
        match self {
            PhpType::Named(s) => {
                if !is_keyword_type(s) && !s.is_empty() && !names.contains(s) {
                    names.push(s.clone());
                }
            }

            PhpType::Nullable(inner) => inner.collect_class_names(names),

            PhpType::Union(types) | PhpType::Intersection(types) => {
                for t in types {
                    t.collect_class_names(names);
                }
            }

            PhpType::Generic(name, args) => {
                if !is_keyword_type(name) && !name.is_empty() && !names.contains(name) {
                    names.push(name.clone());
                }
                for a in args {
                    a.collect_class_names(names);
                }
            }

            PhpType::Array(inner) => inner.collect_class_names(names),

            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
                for e in entries {
                    e.value_type.collect_class_names(names);
                }
            }

            PhpType::Callable {
                params,
                return_type,
                ..
            } => {
                for p in params {
                    p.type_hint.collect_class_names(names);
                }
                if let Some(ret) = return_type {
                    ret.collect_class_names(names);
                }
            }

            PhpType::ClassString(inner) => {
                if let Some(t) = inner {
                    t.collect_class_names(names);
                }
            }

            PhpType::InterfaceString(inner) => {
                if let Some(t) = inner {
                    t.collect_class_names(names);
                }
            }

            PhpType::KeyOf(inner) | PhpType::ValueOf(inner) => {
                inner.collect_class_names(names);
            }

            PhpType::IndexAccess(base, index) => {
                base.collect_class_names(names);
                index.collect_class_names(names);
            }

            PhpType::Conditional {
                condition,
                then_type,
                else_type,
                ..
            } => {
                condition.collect_class_names(names);
                then_type.collect_class_names(names);
                else_type.collect_class_names(names);
            }

            PhpType::Literal(_) | PhpType::Raw(_) | PhpType::IntRange(_, _) => {}
        }
    }

    /// Recursive helper for [`top_level_class_names`].
    ///
    /// Only descends through union, intersection, and nullable
    /// wrappers. Does not recurse into generic args, callable
    /// params/return, shapes, class-string inner types, etc.
    fn collect_top_level_class_names(&self, names: &mut Vec<String>) {
        match self {
            PhpType::Named(s) if !is_keyword_type(s) && !s.is_empty() && !names.contains(s) => {
                names.push(s.clone());
            }

            PhpType::Nullable(inner) => inner.collect_top_level_class_names(names),

            PhpType::Union(types) | PhpType::Intersection(types) => {
                for t in types {
                    t.collect_top_level_class_names(names);
                }
            }

            // For generics, only the base name is top-level.
            // `Collection<int, User>` → `["Collection"]`.
            PhpType::Generic(name, _)
                if !is_keyword_type(name) && !name.is_empty() && !names.contains(name) =>
            {
                names.push(name.clone());
            }

            // `User[]` — the inner type is the top-level class.
            PhpType::Array(inner) => inner.collect_top_level_class_names(names),

            // Shapes, callables, class-string, key-of, value-of,
            // conditionals, literals, int-ranges — no navigable
            // top-level class name.
            _ => {}
        }
    }

    /// Check whether two `PhpType` values refer to the same type,
    /// ignoring namespace qualification differences.
    ///
    /// Returns `true` when the only difference is that one uses a
    /// fully-qualified class name (e.g. `App\Models\User`) while the
    /// other uses the short name (`User`). Handles unions, intersections,
    /// nullable types, and generic parameters.
    /// Whether this type carries structural information beyond a bare
    /// class name or scalar keyword.
    ///
    /// Returns `true` for generics, shapes, arrays, callables,
    /// class-string, key-of, value-of, conditionals, index access,
    /// int ranges, and literals.  Returns `false` for plain `Named`,
    /// `Raw`, and `Nullable(Named(_))`.
    ///
    /// This replaces the `has_type_structure` helper in
    /// `foreach_resolution.rs` and the string-based checks like
    /// `.contains('<')` scattered across the codebase.
    pub fn has_type_structure(&self) -> bool {
        match self {
            PhpType::Named(_) | PhpType::Raw(_) => false,
            PhpType::Nullable(inner) => inner.has_type_structure(),
            PhpType::Union(members) => members.iter().any(|m| m.has_type_structure()),
            PhpType::Intersection(members) => members.iter().any(|m| m.has_type_structure()),
            _ => true,
        }
    }

    /// Whether this type is "informative" — i.e. carries enough detail
    /// to be worth preserving as a resolved type string.
    ///
    /// Returns `true` for generics, shapes, arrays, callables,
    /// class-string, key-of/value-of, conditionals, index access, int
    /// ranges, literals, and named types that are not vague keywords
    /// like `array`, `mixed`, `object`, `void`, `null`, `self`,
    /// `static`, or `$this`.
    ///
    /// Returns `false` for those vague keywords and for `Raw` types
    /// that lack structural markers.
    ///
    /// This replaces `is_informative_type_string()` in
    /// `rhs_resolution.rs`, avoiding a parse→check round-trip when the
    /// caller already has a `PhpType`.
    pub fn is_informative(&self) -> bool {
        match self {
            PhpType::Generic(..) => true,
            PhpType::ArrayShape(..) | PhpType::ObjectShape(..) => true,
            PhpType::Array(..) => true,
            PhpType::Union(members) => members.iter().any(|m| m.is_informative()),
            PhpType::Nullable(inner) => inner.is_informative(),
            PhpType::Intersection(members) => members.iter().any(|m| m.is_informative()),
            PhpType::Named(_) => {
                !(self.is_bare_array()
                    || self.is_mixed()
                    || self.is_object()
                    || self.is_void()
                    || self.is_null()
                    || self.is_self_like())
            }
            PhpType::Callable { .. } => true,
            PhpType::ClassString(..) | PhpType::InterfaceString(..) => true,
            PhpType::KeyOf(..) | PhpType::ValueOf(..) => true,
            PhpType::IndexAccess(..) => true,
            PhpType::Conditional { .. } => true,
            PhpType::IntRange(..) => true,
            PhpType::Literal(..) => true,
            PhpType::Raw(s) => s.contains('<') || s.contains('{') || s.ends_with("[]"),
        }
    }

    /// Whether this type carries generic type parameters (e.g.
    /// `Collection<int, User>`).
    ///
    /// Returns `true` for `Generic`, `Array` (which represents `T[]`),
    /// and composite types that contain a generic member.  Returns
    /// `false` for bare named types like `Collection` without `<…>`.
    ///
    /// This replaces the `.contains('<')` string heuristic with a
    /// structured check.
    pub fn has_type_parameters(&self) -> bool {
        match self {
            PhpType::Generic(..) => true,
            PhpType::Array(..) => true,
            PhpType::Nullable(inner) => inner.has_type_parameters(),
            PhpType::Union(members) | PhpType::Intersection(members) => {
                members.iter().any(|m| m.has_type_parameters())
            }
            _ => false,
        }
    }

    /// Whether this type references any of the given template parameter names.
    ///
    /// Returns `true` when a `Named` leaf matches one of the names in
    /// `template_params`, or when any nested position (union members,
    /// generic args, nullable inner, etc.) does.  This is used to detect
    /// unsubstituted template parameters in method return types so that
    /// hover can swap them with the call-site-substituted version.
    pub fn references_any_template_param(&self, template_params: &[String]) -> bool {
        if template_params.is_empty() {
            return false;
        }
        match self {
            PhpType::Named(name) => template_params.iter().any(|p| p == name),
            PhpType::Nullable(inner) => inner.references_any_template_param(template_params),
            PhpType::Union(members) | PhpType::Intersection(members) => members
                .iter()
                .any(|m| m.references_any_template_param(template_params)),
            PhpType::Generic(name, args) => {
                template_params.iter().any(|p| p == name)
                    || args
                        .iter()
                        .any(|a| a.references_any_template_param(template_params))
            }
            PhpType::Array(inner) => inner.references_any_template_param(template_params),
            PhpType::ClassString(Some(inner)) | PhpType::InterfaceString(Some(inner)) => {
                inner.references_any_template_param(template_params)
            }
            PhpType::KeyOf(inner) | PhpType::ValueOf(inner) => {
                inner.references_any_template_param(template_params)
            }
            PhpType::Conditional {
                condition,
                then_type,
                else_type,
                ..
            } => {
                condition.references_any_template_param(template_params)
                    || then_type.references_any_template_param(template_params)
                    || else_type.references_any_template_param(template_params)
            }
            PhpType::Callable {
                params,
                return_type,
                ..
            } => {
                params
                    .iter()
                    .any(|p| p.type_hint.references_any_template_param(template_params))
                    || return_type
                        .as_ref()
                        .is_some_and(|r| r.references_any_template_param(template_params))
            }
            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => entries
                .iter()
                .any(|e| e.value_type.references_any_template_param(template_params)),
            PhpType::IndexAccess(base, index) => {
                base.references_any_template_param(template_params)
                    || index.references_any_template_param(template_params)
            }
            PhpType::ClassString(None)
            | PhpType::InterfaceString(None)
            | PhpType::IntRange(..)
            | PhpType::Literal(..)
            | PhpType::Raw(..) => false,
        }
    }

    pub fn equivalent(&self, other: &PhpType) -> bool {
        if self == other {
            return true;
        }
        match (self, other) {
            (PhpType::Named(a), PhpType::Named(b)) => {
                Self::short_name_of(a) == Self::short_name_of(b)
            }
            (PhpType::Nullable(a), PhpType::Nullable(b)) => a.equivalent(b),
            // `?X` is equivalent to `X|null` — normalise Nullable to a
            // two-element Union before comparing so that both notations
            // are treated as identical.
            (PhpType::Nullable(inner), PhpType::Union(members))
            | (PhpType::Union(members), PhpType::Nullable(inner)) => {
                let as_union = PhpType::Union(vec![inner.as_ref().clone(), PhpType::null()]);
                as_union.equivalent(&PhpType::Union(members.clone()))
            }
            (PhpType::Union(a), PhpType::Union(b))
            | (PhpType::Intersection(a), PhpType::Intersection(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                // Sort both sides by their shortened display form so
                // that `Foo|null` matches `null|Foo`.
                let mut sa: Vec<String> = a.iter().map(|t| t.shorten().to_string()).collect();
                let mut sb: Vec<String> = b.iter().map(|t| t.shorten().to_string()).collect();
                sa.sort_unstable();
                sb.sort_unstable();
                sa == sb
            }
            (PhpType::Generic(na, aa), PhpType::Generic(nb, ab)) => {
                Self::short_name_of(na) == Self::short_name_of(nb)
                    && aa.len() == ab.len()
                    && aa.iter().zip(ab.iter()).all(|(x, y)| x.equivalent(y))
            }
            (PhpType::Array(a), PhpType::Array(b)) => a.equivalent(b),
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Subtype checking (structural, without class hierarchy)
    // -----------------------------------------------------------------------

    /// Check whether `self` is a structural subtype of `supertype`.
    ///
    /// This performs subtype checks that can be decided from type
    /// structure alone, **without** consulting a class hierarchy.
    /// It handles:
    ///
    /// - Reflexivity: `T <: T`
    /// - `never` is a subtype of everything
    /// - Everything is a subtype of `mixed`
    /// - `null <: ?T` and `T <: ?T`
    /// - `?T` is sugar for `T|null`, normalised before comparison
    /// - `true <: bool`, `false <: bool`
    /// - `int <: float` (PHP's widening)
    /// - Scalar refinement subtypes: `positive-int <: int`,
    ///   `non-empty-string <: string`, `list <: array`, etc.
    /// - `T[] <: array`
    /// - `array{…} <: array`
    /// - Union: `A|B <: C` iff `A <: C` and `B <: C`
    /// - Union supertype: `A <: B|C` iff `A <: B` or `A <: C`
    /// - Intersection: `A&B <: C` iff `A <: C` or `B <: C`
    /// - Intersection supertype: `A <: B&C` iff `A <: B` and `A <: C`
    /// - Generic covariance for read-only containers:
    ///   `array<Tk, Tv> <: array<Tk2, Tv2>` when `Tk <: Tk2` and `Tv <: Tv2`
    /// - `Callable` covariance on return, contravariance on params
    /// - `class-string<T> <: class-string` and `class-string <: string`
    ///
    /// For nominal class relationships (`Cat <: Animal`) the caller must
    /// check the class hierarchy separately. This method returns `false`
    /// for unrelated named types.
    pub fn is_subtype_of(&self, supertype: &PhpType) -> bool {
        // Reflexivity.
        if self == supertype {
            return true;
        }

        // `never` / `no-return` is bottom — subtype of everything.
        if self.is_never() {
            return true;
        }

        // Everything is a subtype of `mixed`.
        if supertype.is_mixed() {
            return true;
        }

        // ── Nullable normalisation ──────────────────────────────────
        // Treat `?T` as `T|null` for uniform handling.
        if let PhpType::Nullable(inner) = self {
            let as_union = PhpType::Union(vec![inner.as_ref().clone(), PhpType::null()]);
            return as_union.is_subtype_of(supertype);
        }
        if let PhpType::Nullable(inner) = supertype {
            let as_union = PhpType::Union(vec![inner.as_ref().clone(), PhpType::null()]);
            return self.is_subtype_of(&as_union);
        }

        // ── Union subtype: every member must be a subtype ───────────
        if let PhpType::Union(members) = self {
            return members.iter().all(|m| m.is_subtype_of(supertype));
        }

        // ── Union supertype: at least one member must accept self ────
        if let PhpType::Union(members) = supertype {
            return members.iter().any(|m| self.is_subtype_of(m));
        }

        // ── Intersection subtype: at least one member suffices ──────
        if let PhpType::Intersection(members) = self {
            return members.iter().any(|m| m.is_subtype_of(supertype));
        }

        // ── Intersection supertype: all members required ────────────
        if let PhpType::Intersection(members) = supertype {
            return members.iter().all(|m| self.is_subtype_of(m));
        }

        // ── Named ↔ Named scalar subtyping ──────────────────────────
        if let (PhpType::Named(sub), PhpType::Named(sup)) = (self, supertype) {
            return is_named_subtype(sub, sup);
        }

        // ── Literal subtyping ───────────────────────────────────────
        if let PhpType::Literal(lit) = self {
            return literal_is_subtype_of(lit, supertype);
        }

        // ── IntRange <: int ─────────────────────────────────────────
        if matches!(self, PhpType::IntRange(..))
            && let PhpType::Named(sup) = supertype
        {
            return matches!(
                sup.to_ascii_lowercase().as_str(),
                "int" | "integer" | "numeric" | "scalar" | "array-key"
            );
        }

        // ── Array slice: T[] <: array ───────────────────────────────
        if let PhpType::Array(inner_sub) = self {
            match supertype {
                PhpType::Named(sup) => {
                    return matches!(sup.to_ascii_lowercase().as_str(), "array" | "iterable");
                }
                PhpType::Array(inner_sup) => {
                    return inner_sub.is_subtype_of(inner_sup);
                }
                PhpType::Generic(name, params) if is_array_like_name(name) => {
                    // T[] <: array<int, T2> when T <: T2
                    if let Some(val) = params.last() {
                        return inner_sub.is_subtype_of(val);
                    }
                }
                _ => {}
            }
        }

        // ── ArrayShape <: array / iterable ──────────────────────────
        if let PhpType::ArrayShape(entries) = self {
            if let PhpType::Named(sup) = supertype {
                return matches!(sup.to_ascii_lowercase().as_str(), "array" | "iterable");
            }

            // ArrayShape <: array<K, V>  (or other generic array-like)
            // Every shape key must be a subtype of K, every value a subtype of V.
            if let PhpType::Generic(name, params) = supertype
                && is_array_like_name(name)
            {
                match params.len() {
                    // array<V> — only check values.
                    1 => {
                        let val_type = &params[0];
                        return entries.iter().all(|e| e.value_type.is_subtype_of(val_type));
                    }
                    // array<K, V> — check both keys and values.
                    2 => {
                        let key_type = &params[0];
                        let val_type = &params[1];
                        return entries.iter().all(|e| {
                            // Determine the key's type: named string keys are
                            // literal-string, positional keys are int.
                            let entry_key_type = match &e.key {
                                Some(k) if k.parse::<i64>().is_ok() => PhpType::int(),
                                Some(_) => PhpType::string(),
                                None => PhpType::int(),
                            };
                            entry_key_type.is_subtype_of(key_type)
                                && e.value_type.is_subtype_of(val_type)
                        });
                    }
                    _ => {}
                }
            }

            // ArrayShape <: T[] — check all values against T.
            if let PhpType::Array(inner) = supertype {
                return entries.iter().all(|e| e.value_type.is_subtype_of(inner));
            }
        }

        // ── ObjectShape <: object ───────────────────────────────────
        if matches!(self, PhpType::ObjectShape(_))
            && let PhpType::Named(sup) = supertype
        {
            return sup.eq_ignore_ascii_case("object");
        }

        // ── Generic covariance (array-like containers) ──────────────
        if let (PhpType::Generic(name_sub, args_sub), PhpType::Generic(name_sup, args_sup)) =
            (self, supertype)
        {
            let base_sub = name_sub.to_ascii_lowercase();
            let base_sup = name_sup.to_ascii_lowercase();

            // Same base or compatible bases (list <: array, etc.)
            let bases_compatible = base_sub == base_sup
                || (is_array_like_name(name_sub) && is_array_like_name(name_sup));

            if bases_compatible && args_sub.len() == args_sup.len() {
                return args_sub
                    .iter()
                    .zip(args_sup.iter())
                    .all(|(s, t)| s.is_subtype_of(t));
            }
        }

        // Generic array-like <: bare `array` / `iterable`
        if let PhpType::Generic(name, _) = self
            && is_array_like_name(name)
            && let PhpType::Named(sup) = supertype
        {
            return matches!(sup.to_ascii_lowercase().as_str(), "array" | "iterable");
        }

        // ── class-string subtyping ──────────────────────────────────
        match (self, supertype) {
            (PhpType::ClassString(_), PhpType::Named(sup))
                if matches!(sup.to_ascii_lowercase().as_str(), "string" | "class-string") =>
            {
                return true;
            }
            (PhpType::ClassString(Some(sub_inner)), PhpType::ClassString(Some(sup_inner))) => {
                return sub_inner.is_subtype_of(sup_inner);
            }
            (PhpType::ClassString(Some(_)), PhpType::ClassString(None)) => {
                return true;
            }
            _ => {}
        }

        // ── interface-string subtyping ──────────────────────────────
        match (self, supertype) {
            (PhpType::InterfaceString(_), PhpType::Named(sup))
                if matches!(
                    sup.to_ascii_lowercase().as_str(),
                    "string" | "class-string" | "interface-string"
                ) =>
            {
                return true;
            }
            _ => {}
        }

        // ── Callable subtyping ──────────────────────────────────────
        if let (
            PhpType::Callable {
                params: params_sub,
                return_type: ret_sub,
                ..
            },
            PhpType::Callable {
                params: params_sup,
                return_type: ret_sup,
                ..
            },
        ) = (self, supertype)
        {
            // Return type is covariant.
            let ret_ok = match (ret_sub, ret_sup) {
                (Some(rs), Some(rp)) => rs.is_subtype_of(rp),
                (_, None) => true,        // supertype has no return constraint
                (None, Some(_)) => false, // sub has no return but super requires one
            };
            // Parameters are contravariant (supertype params must be
            // subtypes of subtype params).
            let params_ok = if params_sub.len() >= params_sup.len() {
                params_sup
                    .iter()
                    .zip(params_sub.iter())
                    .all(|(p_sup, p_sub)| p_sup.type_hint.is_subtype_of(&p_sub.type_hint))
            } else {
                false
            };
            return ret_ok && params_ok;
        }

        // Callable/Closure specification <: callable | Closure | object
        // A callable specification like `Closure(int): void` is always
        // a Closure instance, which is both callable and an object.
        if matches!(self, PhpType::Callable { .. })
            && let PhpType::Named(sup) = supertype
        {
            return matches!(
                sup.to_ascii_lowercase().as_str(),
                "callable" | "closure" | "object"
            );
        }

        // Bare `Closure` or `callable` <: callable specification.
        // A bare `Closure` might have any signature — we cannot prove
        // it violates the specification, so treat it as compatible.
        if let PhpType::Named(sub) = self
            && matches!(sub.to_ascii_lowercase().as_str(), "callable" | "closure")
            && matches!(supertype, PhpType::Callable { .. })
        {
            return true;
        }

        false
    }

    // -----------------------------------------------------------------------
    // Union / intersection simplification
    // -----------------------------------------------------------------------

    /// Return a simplified copy of this type.
    ///
    /// Applies the following normalisations recursively:
    ///
    /// - Deduplicates union and intersection members.
    /// - `true | false` → `bool` (in either order, including with
    ///   extra members).
    /// - Unions containing `mixed` collapse to `mixed`.
    /// - Unions containing both `T` and `null` where `T` is a single
    ///   type collapse to `?T`.
    /// - Scalar refinement absorption: `positive-int | int` → `int`,
    ///   `non-empty-string | string` → `string`, etc.
    /// - Single-member unions/intersections are unwrapped.
    /// - `?T` where `T` is `never` simplifies to `null`.
    /// - Nested unions are flattened (`(A|B)|C` → `A|B|C`).
    /// - Nested intersections are flattened (`(A&B)&C` → `A&B&C`).
    pub fn simplified(&self) -> PhpType {
        match self {
            PhpType::Union(members) => {
                // Recursively simplify members first.
                let mut simplified: Vec<PhpType> = Vec::with_capacity(members.len());
                for m in members {
                    let s = m.simplified();
                    // Flatten nested unions.
                    if let PhpType::Union(inner) = s {
                        simplified.extend(inner);
                    } else {
                        simplified.push(s);
                    }
                }

                // If any member is `mixed`, the whole union is `mixed`.
                if simplified.iter().any(|m| m.is_mixed()) {
                    return PhpType::mixed();
                }

                // Deduplicate (by Display form for simplicity).
                dedup_types(&mut simplified);

                // `true | false` → `bool`.
                simplify_bool_union(&mut simplified);

                // Scalar refinement absorption.
                absorb_scalar_refinements(&mut simplified);

                // Unwrap single-member union.
                if simplified.len() == 1 {
                    return simplified.into_iter().next().unwrap();
                }
                if simplified.is_empty() {
                    return PhpType::never();
                }

                PhpType::Union(simplified)
            }
            PhpType::Intersection(members) => {
                let mut simplified: Vec<PhpType> = Vec::with_capacity(members.len());
                for m in members {
                    let s = m.simplified();
                    // Flatten nested intersections.
                    if let PhpType::Intersection(inner) = s {
                        simplified.extend(inner);
                    } else {
                        simplified.push(s);
                    }
                }

                dedup_types(&mut simplified);

                // If any member is `never`, the intersection is `never`.
                if simplified.iter().any(|m| m.is_never()) {
                    return PhpType::never();
                }

                if simplified.len() == 1 {
                    return simplified.into_iter().next().unwrap();
                }
                if simplified.is_empty() {
                    return PhpType::mixed();
                }

                PhpType::Intersection(simplified)
            }
            PhpType::Nullable(inner) => {
                let s = inner.simplified();
                if s.is_never() || s.is_null() {
                    PhpType::null()
                } else if s.is_mixed() {
                    PhpType::mixed()
                } else {
                    PhpType::Nullable(Box::new(s))
                }
            }
            PhpType::Generic(name, args) => {
                let simplified_args: Vec<PhpType> = args.iter().map(|a| a.simplified()).collect();
                PhpType::Generic(name.clone(), simplified_args)
            }
            PhpType::Array(inner) => PhpType::Array(Box::new(inner.simplified())),
            PhpType::ClassString(inner) => {
                PhpType::ClassString(inner.as_ref().map(|i| Box::new(i.simplified())))
            }
            PhpType::InterfaceString(inner) => {
                PhpType::InterfaceString(inner.as_ref().map(|i| Box::new(i.simplified())))
            }
            PhpType::KeyOf(inner) => PhpType::KeyOf(Box::new(inner.simplified())),
            PhpType::ValueOf(inner) => PhpType::ValueOf(Box::new(inner.simplified())),
            // Leaf types are already simplified.
            _ => self.clone(),
        }
    }

    // -----------------------------------------------------------------------
    // Intersection distribution over unions
    // -----------------------------------------------------------------------

    /// Distribute intersections over unions.
    ///
    /// Transforms `(A|B) & C` into `(A&C) | (B&C)`, producing a
    /// union of intersections (disjunctive normal form for types).
    ///
    /// This is useful for type narrowing: when an intersection type
    /// contains union members, distributing lets each branch be
    /// checked independently.
    ///
    /// If the type is not an intersection containing unions, returns
    /// a clone unchanged. The result is also simplified.
    pub fn distribute_intersection(&self) -> PhpType {
        match self {
            PhpType::Intersection(members) => {
                // Check if any member is a union.
                let has_union = members.iter().any(|m| matches!(m, PhpType::Union(_)));
                if !has_union {
                    return self.clone();
                }

                // Collect each member as a list of alternatives.
                // Non-union members are singleton lists.
                let alternatives: Vec<Vec<PhpType>> = members
                    .iter()
                    .map(|m| match m {
                        PhpType::Union(u) => u.clone(),
                        other => vec![other.clone()],
                    })
                    .collect();

                // Compute the cartesian product to produce union members.
                let mut product: Vec<Vec<PhpType>> = vec![vec![]];
                for alt_set in &alternatives {
                    let mut new_product = Vec::with_capacity(product.len() * alt_set.len());
                    for existing in &product {
                        for alt in alt_set {
                            let mut combo = existing.clone();
                            combo.push(alt.clone());
                            new_product.push(combo);
                        }
                    }
                    product = new_product;
                }

                // Each product element becomes an intersection.
                let union_members: Vec<PhpType> = product
                    .into_iter()
                    .map(|combo| {
                        if combo.len() == 1 {
                            combo.into_iter().next().unwrap()
                        } else {
                            PhpType::Intersection(combo)
                        }
                    })
                    .collect();

                if union_members.len() == 1 {
                    union_members.into_iter().next().unwrap().simplified()
                } else {
                    PhpType::Union(union_members).simplified()
                }
            }
            _ => self.clone(),
        }
    }

    // -----------------------------------------------------------------------
    // Helpers for subtype / simplification
    // -----------------------------------------------------------------------

    /// Whether this type is `never` (bottom type).
    pub fn is_never(&self) -> bool {
        matches!(self, PhpType::Named(s)
            if matches!(s.to_ascii_lowercase().as_str(),
                "never" | "no-return" | "noreturn" | "never-return" | "never-returns"
            )
        )
    }

    /// Whether this type is `mixed` (top type).
    pub fn is_mixed(&self) -> bool {
        matches!(self, PhpType::Named(s) if s.eq_ignore_ascii_case("mixed"))
    }

    /// Whether this type is `void`.
    pub fn is_void(&self) -> bool {
        matches!(self, PhpType::Named(s) if s.eq_ignore_ascii_case("void"))
    }

    /// Whether this type conveys no useful return type information.
    ///
    /// Returns `true` for `mixed`, `void`, and `never` — types that
    /// indicate "no meaningful return" in conditional resolution.
    pub fn is_uninformative_return(&self) -> bool {
        self.is_mixed() || self.is_void() || self.is_never()
    }

    /// Whether this type is a PHP keyword type (scalar, special, or pseudo-type).
    ///
    /// Returns `true` for types like `int`, `string`, `bool`, `array`, `void`,
    /// `mixed`, `never`, `null`, `object`, `callable`, `iterable`, `self`,
    /// `static`, `parent`, `$this`, `resource`, `class-string`, `array-key`,
    /// `scalar`, `numeric`, etc.
    ///
    /// Returns `false` for user-defined class names like `Collection`, `User`,
    /// and for compound types (unions, intersections, generics, shapes, etc.).
    ///
    /// This is the structured equivalent of `is_keyword_type(&str)` — use
    /// this method when you already have a `PhpType` to avoid stringifying
    /// just to check whether it's a keyword.
    pub fn is_keyword(&self) -> bool {
        match self {
            PhpType::Named(name) => is_keyword_type(name),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Self-reference helper (private)
// ---------------------------------------------------------------------------

/// Whether a bare name string is a self-referencing keyword
/// (`self`, `static`, or `$this`), case-insensitive.
///
/// This is the string-only version of [`PhpType::is_self_ref`],
/// used for the base name of `Generic` nodes where we have a
/// `&str` rather than a `&PhpType`.
fn is_self_ref_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "self" | "static" | "$this"
    )
}

// ---------------------------------------------------------------------------
// Subtype helpers (private)
// ---------------------------------------------------------------------------

/// Check structural subtyping between two named types (scalars, keywords).
///
/// This handles PHP's built-in type lattice without class hierarchy lookup:
/// - `never <: T` for all `T`
/// - `T <: mixed` for all `T`
/// - `true <: bool`, `false <: bool`
/// - `int <: float` (widening)
/// - `int <: numeric`, `float <: numeric`
/// - `int <: scalar`, `float <: scalar`, `string <: scalar`, `bool <: scalar`
/// - `int <: array-key`, `string <: array-key`
/// - Refinement subtypes: `positive-int <: int`, `non-empty-string <: string`, etc.
/// - `list <: array`, `non-empty-list <: array`, `non-empty-array <: array`
/// - `callable <: object` is NOT true (callables can be strings/arrays)
fn is_named_subtype(sub: &str, sup: &str) -> bool {
    let sub_raw = sub.strip_prefix('\\').unwrap_or(sub);
    let sup_raw = sup.strip_prefix('\\').unwrap_or(sup);
    let sub_l = sub_raw.to_ascii_lowercase();
    let sup_l = sup_raw.to_ascii_lowercase();

    if sub_l == sup_l {
        return true;
    }

    // Alias normalisation.
    let sub_n = normalize_alias(&sub_l);
    let sup_n = normalize_alias(&sup_l);

    if sub_n == sup_n {
        return true;
    }

    // `never` is bottom.
    if sub_n == "never" {
        return true;
    }

    // `mixed` is top.
    if sup_n == "mixed" {
        return true;
    }

    // `void` is only a subtype of `mixed` (handled above) and itself.
    if sub_n == "void" || sup_n == "void" {
        return false;
    }

    match sup_n {
        // ── bool supertypes ─────────────────────────────────────
        "bool" => matches!(sub_n, "true" | "false"),

        // ── int supertypes ──────────────────────────────────────
        "int" => matches!(
            sub_n,
            "positive-int"
                | "negative-int"
                | "non-positive-int"
                | "non-negative-int"
                | "non-zero-int"
        ),

        // ── float supertypes ────────────────────────────────────
        "float" => matches!(
            sub_n,
            "int"
                | "positive-int"
                | "negative-int"
                | "non-positive-int"
                | "non-negative-int"
                | "non-zero-int"
        ),

        // ── string supertypes ───────────────────────────────────
        "string" => matches!(
            sub_n,
            "non-empty-string"
                | "numeric-string"
                | "class-string"
                | "interface-string"
                | "literal-string"
                | "callable-string"
                | "truthy-string"
                | "non-falsy-string"
                | "trait-string"
                | "enum-string"
                | "lowercase-string"
                | "uppercase-string"
                | "non-empty-lowercase-string"
                | "non-empty-uppercase-string"
                | "non-empty-literal-string"
        ),

        "non-empty-string" | "truthy-string" | "non-falsy-string" => matches!(
            sub_n,
            "non-empty-literal-string"
                | "non-empty-lowercase-string"
                | "non-empty-uppercase-string"
                | "callable-string"
                | "class-string"
                | "interface-string"
                | "trait-string"
                | "enum-string"
        ),

        "literal-string" => matches!(sub_n, "non-empty-literal-string"),

        "lowercase-string" => matches!(sub_n, "non-empty-lowercase-string"),

        "uppercase-string" => matches!(sub_n, "non-empty-uppercase-string"),

        // ── numeric supertypes ──────────────────────────────────
        "numeric" | "number" => matches!(
            sub_n,
            "int"
                | "float"
                | "positive-int"
                | "negative-int"
                | "non-positive-int"
                | "non-negative-int"
                | "non-zero-int"
                | "numeric-string"
        ),

        // ── scalar supertype ────────────────────────────────────
        "scalar" => matches!(
            sub_n,
            "int"
                | "float"
                | "string"
                | "bool"
                | "true"
                | "false"
                | "positive-int"
                | "negative-int"
                | "non-positive-int"
                | "non-negative-int"
                | "non-zero-int"
                | "non-empty-string"
                | "numeric-string"
                | "class-string"
                | "interface-string"
                | "literal-string"
                | "callable-string"
                | "truthy-string"
                | "non-falsy-string"
                | "trait-string"
                | "enum-string"
                | "lowercase-string"
                | "uppercase-string"
                | "non-empty-lowercase-string"
                | "non-empty-uppercase-string"
                | "non-empty-literal-string"
                | "numeric"
                | "number"
        ),

        // ── array-key supertype ─────────────────────────────────
        "array-key" => matches!(
            sub_n,
            "int"
                | "string"
                | "positive-int"
                | "negative-int"
                | "non-positive-int"
                | "non-negative-int"
                | "non-zero-int"
                | "non-empty-string"
                | "numeric-string"
                | "literal-string"
                | "class-string"
                | "interface-string"
                | "callable-string"
                | "truthy-string"
                | "non-falsy-string"
                | "trait-string"
                | "enum-string"
                | "lowercase-string"
                | "uppercase-string"
                | "non-empty-lowercase-string"
                | "non-empty-uppercase-string"
                | "non-empty-literal-string"
        ),

        // ── array supertypes ────────────────────────────────────
        "array" => matches!(
            sub_n,
            "list" | "non-empty-list" | "non-empty-array" | "associative-array"
        ),

        "non-empty-array" => matches!(sub_n, "non-empty-list"),

        // ── iterable supertype ──────────────────────────────────
        "iterable" => matches!(
            sub_n,
            "array" | "list" | "non-empty-array" | "non-empty-list" | "associative-array"
        ),

        // ── object supertype ────────────────────────────────────
        // Every class/interface/enum instance is an object.
        // We use a positive-space check: only names that *look like*
        // class/interface/enum names are accepted.  Unknown pseudo-types
        // fail closed (not a subtype of object) rather than open.
        "object" => matches!(sub_n, "callable-object") || is_class_like_name(sub),

        // ── callable supertype ──────────────────────────────────
        "callable" => matches!(
            sub_n,
            "callable-string" | "callable-array" | "callable-object" | "closure"
        ),

        // ── resource ────────────────────────────────────────────
        "resource" => matches!(sub_n, "closed-resource" | "open-resource"),

        _ => false,
    }
}

/// Normalise common PHP type aliases to a canonical form.
fn normalize_alias(name: &str) -> &str {
    match name {
        "integer" => "int",
        "double" => "float",
        "boolean" => "bool",
        "no-return" | "noreturn" | "never-return" | "never-returns" => "never",
        "non-empty-mixed" => "mixed",
        other => other,
    }
}

/// Check whether a literal type is a subtype of a given supertype.
fn literal_is_subtype_of(lit: &LiteralValue, supertype: &PhpType) -> bool {
    match supertype {
        PhpType::Literal(other_lit) => literals_equal(lit, other_lit),
        PhpType::IntRange(min, max) => lit
            .parse_i64()
            .is_some_and(|value| int_literal_is_within_range(value, min, max)),
        PhpType::Named(sup) => {
            let sup_l = sup.to_ascii_lowercase();
            // Integer literal → int (and its supertypes).
            if matches!(lit, LiteralValue::Int(_)) {
                return matches!(
                    sup_l.as_str(),
                    "int"
                        | "integer"
                        | "float"
                        | "double"
                        | "numeric"
                        | "number"
                        | "scalar"
                        | "array-key"
                );
            }
            // Float literal → float (and its supertypes).
            if matches!(lit, LiteralValue::Float(_)) {
                return matches!(
                    sup_l.as_str(),
                    "float" | "double" | "numeric" | "number" | "scalar"
                );
            }
            // String literal → string (and its supertypes).
            if let Some(content) = lit.string_content() {
                if matches!(
                    sup_l.as_str(),
                    "string" | "literal-string" | "scalar" | "array-key"
                ) {
                    return true;
                }

                // Non-empty string subtypes: any literal with content.
                if !content.is_empty()
                    && matches!(
                        sup_l.as_str(),
                        "non-empty-string" | "non-empty-literal-string"
                    )
                {
                    return true;
                }

                // Truthy/non-falsy string: non-empty and not "0".
                if !content.is_empty()
                    && content != "0"
                    && matches!(sup_l.as_str(), "truthy-string" | "non-falsy-string")
                {
                    return true;
                }

                // Numeric-string: the content parses as a number.
                if sup_l == "numeric-string" && lit.is_numeric_string() {
                    return true;
                }

                return false;
            }
            false
        }
        _ => false,
    }
}

fn literals_equal(left: &LiteralValue, right: &LiteralValue) -> bool {
    match (left, right) {
        (LiteralValue::Int(_), LiteralValue::Int(_)) => left.parse_i64() == right.parse_i64(),
        (LiteralValue::Float(_), LiteralValue::Float(_)) => left.parse_f64() == right.parse_f64(),
        (LiteralValue::String(_), LiteralValue::String(_)) => {
            left.string_content() == right.string_content()
        }
        _ => left == right,
    }
}

pub(crate) fn int_literal_is_within_range(value: i64, min: &str, max: &str) -> bool {
    let min_ok = match min.trim().to_ascii_lowercase().as_str() {
        "min" => true,
        min => min.parse::<i64>().is_ok_and(|bound| value >= bound),
    };
    let max_ok = match max.trim().to_ascii_lowercase().as_str() {
        "max" => true,
        max => max.parse::<i64>().is_ok_and(|bound| value <= bound),
    };

    min_ok && max_ok
}

fn literal_number_type(raw: String) -> PhpType {
    if parse_php_int_literal(&raw).is_some() {
        PhpType::literal_int(raw)
    } else {
        PhpType::literal_float(raw)
    }
}

fn parse_php_int_literal(raw: &str) -> Option<i64> {
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

fn parse_php_float_literal(raw: &str) -> Option<f64> {
    let clean = raw.trim().replace('_', "");
    if clean.is_empty() {
        return None;
    }
    clean.parse::<f64>().ok()
}

// ---------------------------------------------------------------------------
// Simplification helpers (private)
// ---------------------------------------------------------------------------

/// Deduplicate types in a vector by their `Display` form.
fn dedup_types(types: &mut Vec<PhpType>) {
    let mut seen = std::collections::HashSet::new();
    types.retain(|t| {
        let key = t.to_string().to_ascii_lowercase();
        seen.insert(key)
    });
}

/// If a union contains both `true` and `false`, replace them with `bool`.
fn simplify_bool_union(types: &mut Vec<PhpType>) {
    let has_true = types
        .iter()
        .any(|t| matches!(t, PhpType::Named(s) if s.eq_ignore_ascii_case("true")));
    let has_false = types
        .iter()
        .any(|t| matches!(t, PhpType::Named(s) if s.eq_ignore_ascii_case("false")));

    if has_true && has_false {
        types.retain(|t| {
            !matches!(t, PhpType::Named(s)
                if matches!(s.to_ascii_lowercase().as_str(), "true" | "false"))
        });
        types.push(PhpType::bool());
    }
}

/// Absorb scalar refinements into their parent types.
///
/// When a union contains both a refinement and its parent (e.g.
/// `positive-int | int`), the refinement is redundant and removed.
fn absorb_scalar_refinements(types: &mut Vec<PhpType>) {
    // Collect the named types present.
    let named_set: std::collections::HashSet<String> = types
        .iter()
        .filter_map(|t| {
            if let PhpType::Named(s) = t {
                Some(s.to_ascii_lowercase())
            } else {
                None
            }
        })
        .collect();

    if named_set.is_empty() {
        return;
    }

    types.retain(|t| {
        if let PhpType::Named(s) = t {
            let lower = s.to_ascii_lowercase();
            // Check if any OTHER type in the set is a proper supertype.
            for sup in &named_set {
                if sup != &lower && is_named_subtype(&lower, sup) {
                    return false; // Remove: absorbed by the supertype.
                }
            }
        }
        true
    });
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
fn strip_variance_annotations_from_type(s: &str) -> std::borrow::Cow<'_, str> {
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

/// Whether a type name is a keyword that should never be resolved as a
/// class name.
///
/// This is a superset of [`is_scalar_name`] that also includes PHPDoc-only
/// pseudo-types and special names that `resolve_type_string` skips.
pub(crate) fn is_keyword_type(name: &str) -> bool {
    if is_scalar_name(name) {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        // ── Integer refinements ─────────────────────────────────
        "non-zero-int"
            | "int-mask"
            | "int-mask-of"
            // ── String refinements ──────────────────────────────────
            | "literal-string"
            | "callable-string"
            | "uppercase-string"
            | "non-empty-uppercase-string"
            | "non-empty-literal-string"
            // ── Class-string variants ───────────────────────────────
            | "trait-string"
            | "enum-string"
            // ── Array / list refinements ────────────────────────────
            | "associative-array"
        // ── Scalar / mixed variants ─────────────────────────────
            | "mixed"
            | "empty-scalar"
            | "non-empty-scalar"
            | "non-empty-mixed"
            | "number"
            | "empty"
            // ── Object / callable variants ──────────────────────────
            | "callable-object"
            | "callable-array"
            // ── Resource variants ───────────────────────────────────
            | "closed-resource"
            | "open-resource"
            // ── Never aliases ───────────────────────────────────────
            | "no-return"
            | "noreturn"
            | "never-return"
            | "never-returns"
            // ── Key / value projection ──────────────────────────────
            | "key-of"
            | "value-of"
            // ── Special keywords ────────────────────────────────────
            | "class"
    )
}

/// Whether a type name refers to a scalar / built-in type.
/// Narrow primitive scalar check matching built-in PHP types.
pub(crate) fn is_primitive_scalar_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int"
            | "integer"
            | "float"
            | "double"
            | "string"
            | "bool"
            | "boolean"
            | "void"
            | "never"
            | "null"
            | "false"
            | "true"
            | "array"
            | "callable"
            | "iterable"
            | "resource"
    )
}

/// Returns `true` when `name` is a built-in type keyword (case-sensitive) that
/// can never be a class name.  Covers PHP scalar/pseudo types and PHPStan
/// refinement types.
///
/// Only matches exact lowercase forms.  PHP allows classes named `Resource`,
/// `String`, `Object`, etc. (capitalised), so a case-insensitive check would
/// produce false positives.
///
/// Used by class resolution to short-circuit lookups for namespace-qualified
/// type hints like `Tests\Feature\int`, and by type resolution to skip class
/// loading for names that are obviously not classes.
pub(crate) fn is_builtin_non_class_type(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "float"
            | "string"
            | "bool"
            | "array"
            | "object"
            | "null"
            | "void"
            | "never"
            | "mixed"
            | "true"
            | "false"
            | "callable"
            | "iterable"
            | "resource"
            | "numeric"
            | "scalar"
            | "positive-int"
            | "negative-int"
            | "non-negative-int"
            | "non-positive-int"
            | "non-zero-int"
            | "numeric-string"
            | "non-empty-string"
            | "non-falsy-string"
            | "truthy-string"
            | "literal-string"
            | "class-string"
            | "interface-string"
            | "model-property"
            | "array-key"
            | "list"
            | "non-empty-list"
            | "non-empty-array"
            | "empty"
            | "no-return"
            | "never-return"
            | "never-returns"
            | "number"
            | "double"
            | "boolean"
            | "integer"
            | "real"
    )
}

/// Returns `true` for type names that represent array-like types in PHP.
pub fn is_array_like_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "array" | "list" | "non-empty-array" | "non-empty-list" | "iterable"
    )
}

/// Public wrapper around [`is_scalar_name`] for use by other modules
/// (e.g. type-guard narrowing in `narrowing.rs`).
pub fn is_scalar_name_pub(name: &str) -> bool {
    is_scalar_name(name)
}

fn is_scalar_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int"
            | "integer"
            | "float"
            | "double"
            | "string"
            | "bool"
            | "boolean"
            | "void"
            | "never"
            | "null"
            | "false"
            | "true"
            | "array"
            | "callable"
            | "iterable"
            | "resource"
            | "object"
            | "self"
            | "static"
            | "parent"
            | "$this"
            | "class-string"
            | "interface-string"
            | "trait-string"
            | "enum-string"
            | "model-property"
            | "numeric-string"
            | "non-empty-string"
            | "non-empty-lowercase-string"
            | "lowercase-string"
            | "uppercase-string"
            | "non-empty-uppercase-string"
            | "truthy-string"
            | "non-falsy-string"
            | "array-key"
            | "scalar"
            | "numeric"
            | "positive-int"
            | "negative-int"
            | "non-positive-int"
            | "non-negative-int"
            | "non-zero-int"
            | "non-empty-array"
            | "non-empty-list"
            | "list"
            | "associative-array"
            | "callable-string"
            | "callable-array"
            | "callable-object"
            | "literal-string"
            | "non-empty-literal-string"
            | "number"
            | "open-resource"
            | "closed-resource"
    )
}

/// Returns `true` for names that look like user-defined class,
/// interface, or enum names (as opposed to scalar types, keywords,
/// or pseudo-types).
///
/// This is used as a positive-space guard in the `"object"` subtype
/// arm of [`is_named_subtype`]: only names that pass this check are
/// treated as class-like (and therefore subtypes of `object`).
///
/// Names in [`is_scalar_name`] are excluded (not class-like).  Any
/// remaining identifier that starts with a letter or `_` is assumed
/// to be a class name.  This means unknown pseudo-types that are NOT
/// in `is_scalar_name` fail **open** (treated as class-like).  When
/// new PHPStan pseudo-types are introduced, they must be added to
/// `is_scalar_name` to avoid being misclassified as class names.
fn is_class_like_name(name: &str) -> bool {
    // FQN with namespace separator — definitely a class.
    if name.contains('\\') {
        return true;
    }
    // Known scalar/keyword/pseudo-types are not class-like.
    if is_scalar_name(name) {
        return false;
    }
    // After filtering out all known scalars, keywords, and pseudo-types,
    // any remaining name that starts with a valid PHP identifier character
    // is treated as a class name. PHP allows lowercase class names
    // (e.g. finfo, simplexmlelement), so we don't require uppercase.
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

/// Map a PHPStan/docblock type name to its native PHP equivalent.
///
/// Returns `Some("int")` for `positive-int`, `Some("string")` for
/// `class-string`, `Some("array")` for `list`, etc.  Returns `None`
/// for names that have no single native PHP type (`scalar`, `numeric`,
/// `array-key`, `number`).  Class names pass through unchanged.
/// Normalize keyword type casing and PHP aliases to their canonical forms.
///
/// Unlike [`native_scalar_name`], this does **not** collapse PHPStan
/// refinement types (`non-empty-string`, `positive-int`, etc.) to their
/// base PHP types.  It only handles:
///
/// - PHP aliases: `integer` → `int`, `boolean` → `bool`, `double` → `float`
/// - Case normalization: `NULL` → `null`, `TRUE` → `true`, `aRray` → `array`
///
/// Class names and unrecognised identifiers pass through unchanged.
fn normalize_keyword_casing(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        // PHP aliases that map to a different canonical name.
        "integer" => "int".to_string(),
        "boolean" => "bool".to_string(),
        "double" => "float".to_string(),
        "no-return" | "noreturn" | "never-return" | "never-returns" => "never".to_string(),
        // Known keywords — return the lowercased form.
        "int" | "float" | "string" | "bool" | "void" | "never" | "null" | "false" | "true"
        | "mixed" | "object" | "array" | "callable" | "iterable" | "resource" | "self"
        | "static" | "parent"
        // PHPStan refinement types — lowercase but do NOT collapse.
        | "positive-int" | "negative-int" | "non-positive-int" | "non-negative-int"
        | "non-zero-int"
        | "non-empty-string" | "numeric-string" | "class-string" | "interface-string"
        | "literal-string" | "callable-string" | "truthy-string" | "non-falsy-string"
        | "trait-string" | "enum-string" | "lowercase-string" | "uppercase-string"
        | "non-empty-lowercase-string" | "non-empty-uppercase-string"
        | "non-empty-literal-string"
        | "non-empty-array" | "non-empty-list" | "non-empty-mixed" | "associative-array"
        | "closed-resource" | "open-resource" | "callable-object" | "callable-array"
        | "stringable-object"
        | "array-key" | "scalar" | "numeric" | "number" => lower,
        // Not a keyword — return the original name unchanged
        // (preserving class name casing).
        _ => name.to_string(),
    }
}

fn native_scalar_name(name: &str) -> Option<&str> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        // Direct native types.
        "int" | "integer" => Some("int"),
        "float" | "double" => Some("float"),
        "string" => Some("string"),
        "bool" | "boolean" => Some("bool"),
        "void" => Some("void"),
        "never" | "no-return" | "noreturn" | "never-return" | "never-returns" => Some("never"),
        "null" => Some("null"),
        "false" => Some("false"),
        "true" => Some("true"),
        "array" | "non-empty-array" | "list" | "non-empty-list" | "associative-array" => {
            Some("array")
        }
        "callable" | "callable-object" | "callable-array" => Some("callable"),
        "iterable" => Some("iterable"),
        "resource" | "closed-resource" | "open-resource" => Some("resource"),
        "mixed" | "non-empty-mixed" => Some("mixed"),
        "object" => Some("object"),
        "self" => Some("self"),
        "static" | "$this" => Some("static"),
        "parent" => Some("parent"),

        // PHPStan int refinements → int.
        "positive-int" | "negative-int" | "non-positive-int" | "non-negative-int"
        | "non-zero-int" => Some("int"),

        // PHPStan string refinements → string.
        "non-empty-string"
        | "numeric-string"
        | "class-string"
        | "interface-string"
        | "literal-string"
        | "callable-string"
        | "truthy-string"
        | "non-falsy-string"
        | "trait-string"
        | "enum-string"
        | "lowercase-string"
        | "uppercase-string"
        | "non-empty-lowercase-string"
        | "non-empty-uppercase-string"
        | "non-empty-literal-string" => Some("string"),

        // Types with no single native equivalent.
        "scalar" | "numeric" | "number" | "array-key" | "empty-scalar" | "non-empty-scalar"
        | "empty" => None,

        // Anything else is a class name — pass it through.
        _ => Some(name),
    }
}

/// Convert a borrowed mago AST `Type` into an owned `PhpType`.
fn convert(ty: &ast::Type<'_>) -> PhpType {
    match ty {
        // -- Composite types --------------------------------------------------
        ast::Type::Union(_) => {
            let members = flatten_union(ty);
            PhpType::Union(members)
        }
        ast::Type::Intersection(_) => {
            let members = flatten_intersection(ty);
            PhpType::Intersection(members)
        }
        ast::Type::Nullable(n) => PhpType::Nullable(Box::new(convert(n.inner))),
        ast::Type::Parenthesized(p) => convert(p.inner),

        // -- Named / Reference types ------------------------------------------
        ast::Type::Reference(r) => {
            let name = bytes_to_str(r.identifier.value).to_string();
            match &r.parameters {
                Some(params) => {
                    let args: Vec<PhpType> =
                        params.entries.iter().map(|e| convert(&e.inner)).collect();
                    PhpType::Generic(name, args)
                }
                None => PhpType::Named(name),
            }
        }

        // -- Array-like types with optional generic parameters ----------------
        ast::Type::Array(a) => {
            convert_keyword_with_optional_generics(bytes_to_str(a.keyword.value), &a.parameters)
        }
        ast::Type::NonEmptyArray(a) => {
            convert_keyword_with_optional_generics(bytes_to_str(a.keyword.value), &a.parameters)
        }
        ast::Type::AssociativeArray(a) => {
            convert_keyword_with_optional_generics(bytes_to_str(a.keyword.value), &a.parameters)
        }
        ast::Type::List(l) => {
            convert_keyword_with_optional_generics(bytes_to_str(l.keyword.value), &l.parameters)
        }
        ast::Type::NonEmptyList(l) => {
            convert_keyword_with_optional_generics(bytes_to_str(l.keyword.value), &l.parameters)
        }
        ast::Type::Iterable(i) => {
            convert_keyword_with_optional_generics(bytes_to_str(i.keyword.value), &i.parameters)
        }

        // -- Slice: T[] -------------------------------------------------------
        ast::Type::Slice(s) => PhpType::Array(Box::new(convert(s.inner))),

        // -- Shape types ------------------------------------------------------
        ast::Type::Shape(s) => {
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
                ast::ShapeTypeKind::Array
                | ast::ShapeTypeKind::NonEmptyArray
                | ast::ShapeTypeKind::AssociativeArray
                | ast::ShapeTypeKind::List
                | ast::ShapeTypeKind::NonEmptyList => PhpType::ArrayShape(entries),
            }
        }

        // -- Object type (with optional shape) --------------------------------
        ast::Type::Object(o) => match &o.properties {
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
        ast::Type::Callable(c) => {
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
        ast::Type::Conditional(c) => PhpType::Conditional {
            param: c.subject.to_string(),
            negated: c.is_negated(),
            condition: Box::new(convert(c.target)),
            then_type: Box::new(convert(c.then)),
            else_type: Box::new(convert(c.otherwise)),
        },

        // -- class-string / interface-string ----------------------------------
        ast::Type::ClassString(c) => {
            let inner = c
                .parameter
                .as_ref()
                .map(|p| Box::new(convert(&p.entry.inner)));
            PhpType::ClassString(inner)
        }
        ast::Type::InterfaceString(i) => {
            let inner = i
                .parameter
                .as_ref()
                .map(|p| Box::new(convert(&p.entry.inner)));
            PhpType::InterfaceString(inner)
        }

        // -- key-of / value-of ------------------------------------------------
        ast::Type::KeyOf(k) => PhpType::KeyOf(Box::new(convert(&k.parameter.entry.inner))),
        ast::Type::ValueOf(v) => PhpType::ValueOf(Box::new(convert(&v.parameter.entry.inner))),

        // -- int range --------------------------------------------------------
        ast::Type::IntRange(r) => PhpType::IntRange(r.min.to_string(), r.max.to_string()),

        // -- Index access: T[K] -----------------------------------------------
        ast::Type::IndexAccess(i) => {
            PhpType::IndexAccess(Box::new(convert(i.target)), Box::new(convert(i.index)))
        }

        // -- Variable (e.g. $this in conditional types) -----------------------
        ast::Type::Variable(v) => PhpType::Named(bytes_to_str(v.value).to_string()),

        // -- Literal types ----------------------------------------------------
        ast::Type::LiteralInt(l) => PhpType::literal_int(bytes_to_str(l.raw).to_string()),
        ast::Type::LiteralFloat(l) => PhpType::literal_float(bytes_to_str(l.raw).to_string()),
        ast::Type::LiteralString(l) => PhpType::literal_string_raw(bytes_to_str(l.raw).to_string()),

        // -- Negated / Posited literals (e.g. -42, +42) -----------------------
        ast::Type::Negated(n) => literal_number_type(format!("-{}", n.number)),
        ast::Type::Posited(p) => literal_number_type(format!("+{}", p.number)),

        // -- Keyword types → Named -------------------------------------------
        ast::Type::Mixed(k)
        | ast::Type::NonEmptyMixed(k)
        | ast::Type::Null(k)
        | ast::Type::Void(k)
        | ast::Type::Never(k)
        | ast::Type::Resource(k)
        | ast::Type::ClosedResource(k)
        | ast::Type::OpenResource(k)
        | ast::Type::True(k)
        | ast::Type::False(k)
        | ast::Type::Bool(k)
        | ast::Type::Float(k)
        | ast::Type::Int(k)
        | ast::Type::PositiveInt(k)
        | ast::Type::NegativeInt(k)
        | ast::Type::NonPositiveInt(k)
        | ast::Type::NonNegativeInt(k)
        | ast::Type::String(k)
        | ast::Type::StringableObject(k)
        | ast::Type::ArrayKey(k)
        | ast::Type::Numeric(k)
        | ast::Type::Scalar(k)
        | ast::Type::NumericString(k)
        | ast::Type::NonEmptyString(k)
        | ast::Type::NonEmptyLowercaseString(k)
        | ast::Type::LowercaseString(k)
        | ast::Type::NonEmptyUppercaseString(k)
        | ast::Type::UppercaseString(k)
        | ast::Type::TruthyString(k)
        | ast::Type::NonFalsyString(k)
        | ast::Type::UnspecifiedLiteralInt(k)
        | ast::Type::UnspecifiedLiteralString(k)
        | ast::Type::UnspecifiedLiteralFloat(k)
        | ast::Type::NonEmptyUnspecifiedLiteralString(k) => {
            PhpType::Named(normalize_keyword_casing(bytes_to_str(k.value)))
        }

        // -- Catch-all for anything else (non_exhaustive) ---------------------
        other => PhpType::Raw(other.to_string()),
    }
}

/// Convert a keyword type that has optional generic parameters (like
/// `array`, `array<int>`, `list<string>`, `non-empty-array<int, string>`,
/// `iterable<K, V>`).
fn convert_keyword_with_optional_generics(
    keyword: &str,
    parameters: &Option<ast::GenericParameters<'_>>,
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
fn flatten_union(ty: &ast::Type<'_>) -> Vec<PhpType> {
    match ty {
        ast::Type::Union(u) => {
            let mut types = flatten_union(u.left);
            types.extend(flatten_union(u.right));
            types
        }
        other => vec![convert(other)],
    }
}

/// Recursively flatten a left-leaning binary intersection tree into a flat `Vec`.
fn flatten_intersection(ty: &ast::Type<'_>) -> Vec<PhpType> {
    match ty {
        ast::Type::Intersection(i) => {
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
fn evaluate_key_of(resolved: &PhpType) -> PhpType {
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
fn evaluate_value_of(resolved: &PhpType) -> PhpType {
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
fn evaluate_index_access(base: &PhpType, index: &PhpType) -> PhpType {
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

fn literal_or_named_shape_key(ty: &PhpType) -> Option<String> {
    match ty {
        PhpType::Literal(lit) => lit.string_content().map(ToOwned::to_owned),
        PhpType::Named(key) => Some(key.clone()),
        _ => None,
    }
}

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a type string with mago to get the canonical display form,
    /// then parse with `PhpType::parse()` and verify our Display matches.
    ///
    /// For types where mago's `Display` has a known bug (double angle
    /// brackets on `class-string`, `interface-string`, `key-of`,
    /// `value-of`), use [`assert_round_trip_expected`] instead.
    fn assert_round_trip(input: &str) {
        // First, get mago's canonical output.
        let span = Span::new(
            FileId::zero(),
            Position::new(0),
            Position::new(input.len() as u32),
        );
        let arena = Bump::new();
        let input_arena = arena.alloc_slice_copy(input.as_bytes());
        let mago_canonical = match mago_type_syntax::parse_str(&arena, span, input_arena) {
            Ok(ty) => ty.to_string(),
            Err(_) => {
                // If mago can't parse it, PhpType should produce Raw.
                let php_type = PhpType::parse(input);
                assert_eq!(
                    php_type,
                    PhpType::Raw(input.to_owned()),
                    "Unparseable input should become Raw"
                );
                return;
            }
        };

        let php_type = PhpType::parse(input);
        let our_output = php_type.to_string();
        assert_eq!(
            our_output, mago_canonical,
            "Round-trip mismatch for input: {input:?}\n  PhpType:  {php_type:?}\n  ours:     {our_output:?}\n  mago:     {mago_canonical:?}"
        );
    }

    /// Like [`assert_round_trip`] but compares against an explicit expected
    /// string instead of mago's `Display` output.  Used to work around a
    /// mago Display bug where `SingleGenericParameter` wraps the entry in
    /// `<>` and then the parent type adds another pair, producing double
    /// angle brackets (e.g. `class-string<<Foo>>`).
    fn assert_round_trip_expected(input: &str, expected: &str) {
        let php_type = PhpType::parse(input);
        let our_output = php_type.to_string();
        assert_eq!(
            our_output, expected,
            "Round-trip mismatch for input: {input:?}\n  PhpType:  {php_type:?}\n  ours:     {our_output:?}\n  expected: {expected:?}"
        );
    }

    #[test]
    fn round_trip_keywords() {
        let keywords = [
            "int",
            "string",
            "bool",
            "float",
            "mixed",
            "void",
            "null",
            "never",
            "true",
            "false",
            "object",
            "array",
            "callable",
            "iterable",
            "self",
            "static",
            "parent",
            "resource",
            "positive-int",
            "negative-int",
            "non-empty-string",
            "numeric-string",
            "array-key",
        ];
        for kw in keywords {
            assert_round_trip(kw);
        }
    }

    #[test]
    fn round_trip_nullable() {
        assert_round_trip("?int");
        assert_round_trip("?string");
        assert_round_trip("?Foo");
    }

    #[test]
    fn round_trip_union() {
        // mago Display uses spaced unions (`int | string`); we prefer
        // the PHP convention (`int|string`).
        assert_round_trip_expected("int|string", "int|string");
        assert_round_trip_expected("int|string|null", "int|string|null");
        assert_round_trip_expected("Foo|Bar|null", "Foo|Bar|null");
        assert_round_trip_expected("int|null", "int|null");
    }

    #[test]
    fn round_trip_intersection() {
        // mago Display uses spaced intersections (`Countable & Traversable`);
        // we prefer the PHP convention (`Countable&Traversable`).
        assert_round_trip_expected("Countable&Traversable", "Countable&Traversable");
    }

    #[test]
    fn round_trip_generics() {
        assert_round_trip("array<int, string>");
        assert_round_trip("array<string>");
        assert_round_trip("Collection<int, User>");
        assert_round_trip("list<int>");
        assert_round_trip("non-empty-list<string>");
        assert_round_trip("non-empty-array<string>");
    }

    #[test]
    fn parse_generic_with_covariant_this() {
        // Laravel/Larastan uses `covariant $this` in generic args, e.g.
        // `BelongsTo<Category, covariant $this>`.  The parser should
        // still extract the base class name (`BelongsTo`) so that
        // member lookup works on the relationship class.
        let ty = PhpType::parse("BelongsTo<Category, covariant $this>");
        let base = ty.base_name();
        assert_eq!(
            base,
            Some("BelongsTo"),
            "base_name should be 'BelongsTo' even with 'covariant $this' arg, got: {:?} from {:?}",
            base,
            ty,
        );
    }

    #[test]
    fn parse_generic_with_covariant_preserves_structure() {
        // The full Generic structure should be preserved after stripping.
        let ty = PhpType::parse("HasMany<Post, covariant $this>");
        match &ty {
            PhpType::Generic(name, args) => {
                assert_eq!(name, "HasMany");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0].to_string(), "Post");
                assert_eq!(args[1].to_string(), "$this");
            }
            other => panic!("expected Generic, got: {:?}", other),
        }
    }

    #[test]
    fn parse_generic_with_contravariant() {
        let ty = PhpType::parse("Comparator<contravariant T>");
        assert_eq!(
            ty.base_name(),
            Some("Comparator"),
            "base_name should work with contravariant annotation",
        );
        match &ty {
            PhpType::Generic(_, args) => {
                assert_eq!(args.len(), 1);
                assert_eq!(args[0].to_string(), "T");
            }
            other => panic!("expected Generic, got: {:?}", other),
        }
    }

    #[test]
    fn parse_generic_with_covariant_fqn() {
        // Fully-qualified relationship type with covariant $this.
        let ty = PhpType::parse(
            "Illuminate\\Database\\Eloquent\\Relations\\BelongsTo<Category, covariant $this>",
        );
        assert_eq!(
            ty.base_name(),
            Some("Illuminate\\Database\\Eloquent\\Relations\\BelongsTo"),
        );
    }

    #[test]
    fn parse_generic_with_multiple_covariant_args() {
        let ty = PhpType::parse("Map<covariant TKey, covariant TValue>");
        match &ty {
            PhpType::Generic(name, args) => {
                assert_eq!(name, "Map");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0].to_string(), "TKey");
                assert_eq!(args[1].to_string(), "TValue");
            }
            other => panic!("expected Generic, got: {:?}", other),
        }
    }

    #[test]
    fn parse_no_false_strip_of_covariant_class_name() {
        // A class named `covariant` (unlikely but possible) should not
        // be stripped when it is NOT inside a generic parameter position.
        // It appears at the top level, not after `<` or `,`.
        let ty = PhpType::parse("covariant");
        // mago may or may not parse this as a Named type; the key is
        // that stripping should NOT remove it since it's not after < or ,.
        assert_ne!(ty.to_string(), "", "should not produce empty string");
    }

    #[test]
    fn parse_generic_without_covariant_unchanged() {
        // Normal generics without variance annotations should be unaffected.
        let ty = PhpType::parse("Collection<int, User>");
        match &ty {
            PhpType::Generic(name, args) => {
                assert_eq!(name, "Collection");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected Generic, got: {:?}", other),
        }
    }

    #[test]
    fn parse_covariant_array_shape_in_generic() {
        // `covariant array{...}` inside a generic — the array shape
        // should still parse after stripping the variance keyword.
        let ty = PhpType::parse(
            "Collection<int, covariant array{customer: Customer, contact: Contact|null}>",
        );
        assert_eq!(ty.base_name(), Some("Collection"));
        match &ty {
            PhpType::Generic(_, args) => {
                assert_eq!(args.len(), 2);
                // The second arg should be an array shape, not Raw.
                assert!(
                    matches!(&args[1], PhpType::ArrayShape(_)),
                    "second arg should be ArrayShape after stripping covariant, got: {:?}",
                    args[1],
                );
            }
            other => panic!("expected Generic, got: {:?}", other),
        }
    }

    #[test]
    fn round_trip_class_references() {
        assert_round_trip("Foo\\Bar");
        assert_round_trip("\\Foo\\Bar");
    }

    #[test]
    fn round_trip_shapes() {
        assert_round_trip("array{name: string, age: int}");
        assert_round_trip("array{0: string, 1: int}");
        assert_round_trip("array{name?: string}");
        assert_round_trip("object{name: string}");
    }

    #[test]
    fn round_trip_callables() {
        assert_round_trip("callable(int, string): bool");
        assert_round_trip("Closure(int): void");
        assert_round_trip("Closure(int, string): void");
        assert_round_trip("callable(): void");
    }

    #[test]
    fn round_trip_class_string() {
        // mago Display bug: class-string<Foo> → class-string<<Foo>>
        assert_round_trip_expected("class-string<Foo>", "class-string<Foo>");
        assert_round_trip("class-string");
    }

    #[test]
    fn round_trip_interface_string() {
        // mago Display bug: interface-string<Foo> → interface-string<<Foo>>
        assert_round_trip_expected("interface-string<Foo>", "interface-string<Foo>");
    }

    #[test]
    fn round_trip_key_of_value_of() {
        // mago Display bug: key-of<T> → key-of<<T>>, value-of<T> → value-of<<T>>
        assert_round_trip_expected("key-of<T>", "key-of<T>");
        assert_round_trip_expected("value-of<T>", "value-of<T>");
    }

    #[test]
    fn round_trip_int_range() {
        assert_round_trip("int<0, 100>");
        assert_round_trip("int<min, max>");
        assert_round_trip("int<0, max>");
    }

    #[test]
    fn round_trip_slice() {
        // `Foo[]` is parsed as `PhpType::Array(Foo)` which displays as `array<Foo>`.
        assert_round_trip_expected("Foo[]", "array<Foo>");
    }

    #[test]
    fn round_trip_literals() {
        assert_round_trip("42");
        assert_round_trip("'foo'");
    }

    #[test]
    fn round_trip_conditional() {
        assert_round_trip("$this is string ? int : float");
    }

    #[test]
    fn round_trip_member_reference() {
        assert_round_trip("Foo::BAR");
        assert_round_trip("Foo::*");
    }

    #[test]
    fn parse_generic_with_star_wildcard() {
        let ty = PhpType::parse("Relation<TRelatedModel, *, *>");
        match &ty {
            PhpType::Generic(name, args) => {
                assert_eq!(name, "Relation");
                assert_eq!(args.len(), 3);
                assert_eq!(args[0], PhpType::Named("TRelatedModel".to_owned()));
                assert_eq!(args[1], PhpType::mixed());
                assert_eq!(args[2], PhpType::mixed());
            }
            other => panic!("Expected Generic, got {:?}", other),
        }
    }

    #[test]
    fn parse_generic_with_star_wildcard_union() {
        // `Relation<TRelatedModel, *, *>|string` should parse as a union
        let ty = PhpType::parse("Relation<TRelatedModel, *, *>|string");
        match &ty {
            PhpType::Union(members) => {
                assert_eq!(members.len(), 2);
                match &members[0] {
                    PhpType::Generic(name, args) => {
                        assert_eq!(name, "Relation");
                        assert_eq!(args.len(), 3);
                    }
                    other => panic!("Expected Generic, got {:?}", other),
                }
                assert_eq!(members[1], PhpType::string());
            }
            other => panic!("Expected Union, got {:?}", other),
        }
    }

    #[test]
    fn parse_generic_star_does_not_mangle_member_reference() {
        // `Foo::*` is a member reference, not a generic wildcard
        assert_round_trip("Foo::*");
    }

    #[test]
    fn replace_star_wildcards_does_not_mangle_constant_pattern() {
        // `int-mask-of<self::FOO_*>` — the `*` is part of a constant
        // pattern, not a generic wildcard (preceded by `_`, not `<`/`,`).
        // Our pre-processing must leave it untouched.  (mago itself may
        // or may not parse the result, but that's a separate issue.)
        use super::replace_star_wildcards;
        let result = replace_star_wildcards("int-mask-of<self::FOO_*>");
        assert_eq!(result.as_ref(), "int-mask-of<self::FOO_*>");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn parse_generic_star_with_spaces() {
        // Spaces around the `*` wildcard
        let ty = PhpType::parse("BelongsTo< * , * >");
        match &ty {
            PhpType::Generic(name, args) => {
                assert_eq!(name, "BelongsTo");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], PhpType::mixed());
                assert_eq!(args[1], PhpType::mixed());
            }
            other => panic!("Expected Generic, got {:?}", other),
        }
    }

    #[test]
    fn replace_star_wildcards_preserves_multibyte() {
        // A multibyte character alongside a generic wildcard must survive
        // the rewrite intact (not be mangled byte-by-byte).
        use super::replace_star_wildcards;
        let result = replace_star_wildcards("Map<Café, *>");
        assert_eq!(result.as_ref(), "Map<Café, mixed>");
    }

    #[test]
    fn strip_variance_annotations_preserves_multibyte() {
        use super::strip_variance_annotations_from_type;
        let result = strip_variance_annotations_from_type("Map<café, covariant Naïve>");
        assert_eq!(result.as_ref(), "Map<café, Naïve>");
    }

    #[test]
    fn replace_star_wildcards_no_star() {
        use super::replace_star_wildcards;
        let result = replace_star_wildcards("Collection<int, User>");
        assert_eq!(result.as_ref(), "Collection<int, User>");
        // Should borrow, not allocate
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn replace_star_wildcards_member_ref() {
        use super::replace_star_wildcards;
        let result = replace_star_wildcards("Foo::*");
        assert_eq!(result.as_ref(), "Foo::*");
        // Should borrow, not allocate
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn replace_star_wildcards_constant_pattern() {
        use super::replace_star_wildcards;
        let result = replace_star_wildcards("int-mask-of<self::FOO_*>");
        assert_eq!(result.as_ref(), "int-mask-of<self::FOO_*>");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn replace_star_wildcards_generic() {
        use super::replace_star_wildcards;
        let result = replace_star_wildcards("Relation<TRelatedModel, *, *>");
        assert_eq!(result.as_ref(), "Relation<TRelatedModel, mixed, mixed>");
    }

    #[test]
    fn replace_star_wildcards_single_star() {
        use super::replace_star_wildcards;
        let result = replace_star_wildcards("Voter<self::*>");
        // `self::*` — the `*` is preceded by `::`, not `<` or `,`
        assert_eq!(result.as_ref(), "Voter<self::*>");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn parse_empty_returns_raw() {
        assert_eq!(PhpType::parse(""), PhpType::Raw(String::new()));
    }

    #[test]
    fn parse_garbage_returns_raw() {
        let php_type = PhpType::parse("|||");
        assert!(matches!(php_type, PhpType::Raw(_)));
    }

    #[test]
    fn union_is_flattened() {
        let ty = PhpType::parse("int|string|null");
        match ty {
            PhpType::Union(members) => {
                assert_eq!(members.len(), 3);
                assert_eq!(members[0], PhpType::int());
                assert_eq!(members[1], PhpType::string());
                assert_eq!(members[2], PhpType::null());
            }
            other => panic!("Expected Union, got {other:?}"),
        }
    }

    #[test]
    fn intersection_is_flattened() {
        let ty = PhpType::parse("A&B&C");
        match ty {
            PhpType::Intersection(members) => {
                assert_eq!(members.len(), 3);
                assert_eq!(members[0], PhpType::Named("A".to_owned()));
                assert_eq!(members[1], PhpType::Named("B".to_owned()));
                assert_eq!(members[2], PhpType::Named("C".to_owned()));
            }
            other => panic!("Expected Intersection, got {other:?}"),
        }
    }

    #[test]
    fn generic_with_params() {
        let ty = PhpType::parse("array<int, string>");
        match ty {
            PhpType::Generic(name, args) => {
                assert_eq!(name, "array");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], PhpType::int());
                assert_eq!(args[1], PhpType::string());
            }
            other => panic!("Expected Generic, got {other:?}"),
        }
    }

    #[test]
    fn class_string_with_param() {
        let ty = PhpType::parse("class-string<Foo>");
        match ty {
            PhpType::ClassString(Some(inner)) => {
                assert_eq!(*inner, PhpType::Named("Foo".to_owned()));
            }
            other => panic!("Expected ClassString(Some), got {other:?}"),
        }
    }

    #[test]
    fn nullable_structure() {
        let ty = PhpType::parse("?int");
        match ty {
            PhpType::Nullable(inner) => {
                assert_eq!(*inner, PhpType::int());
            }
            other => panic!("Expected Nullable, got {other:?}"),
        }
    }

    #[test]
    fn callable_structure() {
        let ty = PhpType::parse("callable(int, string): bool");
        match ty {
            PhpType::Callable {
                kind,
                params,
                return_type,
            } => {
                assert_eq!(kind, "callable");
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].type_hint, PhpType::int());
                assert_eq!(params[1].type_hint, PhpType::string());
                assert_eq!(return_type, Some(Box::new(PhpType::bool())));
            }
            other => panic!("Expected Callable, got {other:?}"),
        }
    }

    #[test]
    fn shape_structure() {
        let ty = PhpType::parse("array{name: string, age?: int}");
        match ty {
            PhpType::ArrayShape(entries) => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].key, Some("name".to_owned()));
                assert_eq!(entries[0].value_type, PhpType::string());
                assert!(!entries[0].optional);
                assert_eq!(entries[1].key, Some("age".to_owned()));
                assert_eq!(entries[1].value_type, PhpType::int());
                assert!(entries[1].optional);
            }
            other => panic!("Expected ArrayShape, got {other:?}"),
        }
    }

    #[test]
    fn shape_value_type_named_key() {
        let ty = PhpType::parse("array{name: string, user: User}");
        assert_eq!(
            ty.shape_value_type("user"),
            Some(&PhpType::Named("User".to_owned()))
        );
        assert_eq!(ty.shape_value_type("name"), Some(&PhpType::string()));
        assert_eq!(ty.shape_value_type("missing"), None);
    }

    #[test]
    fn shape_value_type_positional() {
        let ty = PhpType::parse("array{User, Address}");
        assert_eq!(
            ty.shape_value_type("0"),
            Some(&PhpType::Named("User".to_owned()))
        );
        assert_eq!(
            ty.shape_value_type("1"),
            Some(&PhpType::Named("Address".to_owned()))
        );
        assert_eq!(ty.shape_value_type("2"), None);
    }

    #[test]
    fn shape_value_type_explicit_numeric_key() {
        let ty = PhpType::parse("array{0: User, 1: Address}");
        assert_eq!(
            ty.shape_value_type("0"),
            Some(&PhpType::Named("User".to_owned()))
        );
        assert_eq!(
            ty.shape_value_type("1"),
            Some(&PhpType::Named("Address".to_owned()))
        );
    }

    #[test]
    fn shape_value_type_nullable() {
        let ty = PhpType::parse("?array{name: string}");
        assert_eq!(ty.shape_value_type("name"), Some(&PhpType::string()));
    }

    #[test]
    fn shape_value_type_union_of_shapes() {
        // Union where only one member has the key (conditional shape addition).
        let ty = PhpType::parse("array{name: string}|array{name: string, config: Config}");
        assert_eq!(
            ty.shape_value_type("config"),
            Some(&PhpType::Named("Config".to_owned()))
        );
        // Key present in both members returns the first match.
        assert_eq!(ty.shape_value_type("name"), Some(&PhpType::string()));
        // Key absent from all members.
        assert_eq!(ty.shape_value_type("missing"), None);
    }

    #[test]
    fn shape_value_type_non_shape_returns_none() {
        assert_eq!(
            PhpType::parse("array<int, User>").shape_value_type("0"),
            None
        );
        assert_eq!(PhpType::parse("string").shape_value_type("0"), None);
    }

    #[test]
    fn shape_entries_array() {
        let ty = PhpType::parse("array{name: string, age?: int}");
        let entries = ty.shape_entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, Some("name".to_owned()));
        assert!(!entries[0].optional);
        assert_eq!(entries[1].key, Some("age".to_owned()));
        assert!(entries[1].optional);
    }

    #[test]
    fn shape_entries_object() {
        let ty = PhpType::parse("object{foo: int}");
        let entries = ty.shape_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, Some("foo".to_owned()));
    }

    #[test]
    fn shape_entries_non_shape_returns_none() {
        assert!(PhpType::parse("string").shape_entries().is_none());
        assert!(PhpType::parse("array<int>").shape_entries().is_none());
    }

    #[test]
    fn is_array_shape_test() {
        assert!(PhpType::parse("array{name: string}").is_array_shape());
        assert!(PhpType::parse("?array{name: string}").is_array_shape());
        assert!(!PhpType::parse("array<int>").is_array_shape());
        assert!(!PhpType::parse("object{name: string}").is_array_shape());
    }

    #[test]
    fn is_object_shape_test() {
        assert!(PhpType::parse("object{name: string}").is_object_shape());
        assert!(PhpType::parse("?object{name: string}").is_object_shape());
        assert!(!PhpType::parse("array{name: string}").is_object_shape());
        assert!(!PhpType::parse("string").is_object_shape());
    }

    #[test]
    fn object_shape_property_type_test() {
        let ty = PhpType::parse("object{name: string, user: User}");
        assert_eq!(
            ty.object_shape_property_type("user"),
            Some(&PhpType::Named("User".to_owned()))
        );
        assert_eq!(
            ty.object_shape_property_type("name"),
            Some(&PhpType::string())
        );
        assert_eq!(ty.object_shape_property_type("missing"), None);
    }

    #[test]
    fn object_shape_structure() {
        let ty = PhpType::parse("object{name: string}");
        match ty {
            PhpType::ObjectShape(entries) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].key, Some("name".to_owned()));
                assert_eq!(entries[0].value_type, PhpType::string());
            }
            other => panic!("Expected ObjectShape, got {other:?}"),
        }
    }

    #[test]
    fn slice_structure() {
        let ty = PhpType::parse("Foo[]");
        match ty {
            PhpType::Array(inner) => {
                assert_eq!(*inner, PhpType::Named("Foo".to_owned()));
            }
            other => panic!("Expected Array (slice), got {other:?}"),
        }
    }

    #[test]
    fn conditional_structure() {
        let ty = PhpType::parse("$this is string ? int : float");
        match ty {
            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => {
                assert_eq!(param, "$this");
                assert!(!negated);
                assert_eq!(*condition, PhpType::string());
                assert_eq!(*then_type, PhpType::int());
                assert_eq!(*else_type, PhpType::float());
            }
            other => panic!("Expected Conditional, got {other:?}"),
        }
    }

    #[test]
    fn int_range_structure() {
        let ty = PhpType::parse("int<0, 100>");
        match ty {
            PhpType::IntRange(min, max) => {
                assert_eq!(min, "0");
                assert_eq!(max, "100");
            }
            other => panic!("Expected IntRange, got {other:?}"),
        }
    }

    #[test]
    fn key_of_structure() {
        let ty = PhpType::parse("key-of<T>");
        match ty {
            PhpType::KeyOf(inner) => {
                assert_eq!(*inner, PhpType::Named("T".to_owned()));
            }
            other => panic!("Expected KeyOf, got {other:?}"),
        }
    }

    #[test]
    fn value_of_structure() {
        let ty = PhpType::parse("value-of<T>");
        match ty {
            PhpType::ValueOf(inner) => {
                assert_eq!(*inner, PhpType::Named("T".to_owned()));
            }
            other => panic!("Expected ValueOf, got {other:?}"),
        }
    }

    #[test]
    fn value_of_shape_dedups_non_adjacent_values() {
        // `value-of<array{a: int, b: string, c: int}>` must collapse the
        // two non-adjacent `int` values into a single union member.
        let ty = PhpType::parse("value-of<array{a: int, b: string, c: int}>");
        // `substitute` short-circuits on an empty map, so pass an unrelated
        // binding to force the `value-of` evaluation path to run.
        let subs =
            std::collections::HashMap::from([("T".to_string(), PhpType::Named("int".to_string()))]);
        let evaluated = ty.substitute(&subs);
        match &evaluated {
            PhpType::Union(members) => {
                assert_eq!(
                    members.len(),
                    2,
                    "expected int|string (deduped), got {evaluated:?}"
                );
            }
            other => panic!("Expected a 2-member union, got {other:?}"),
        }
    }

    #[test]
    fn literal_int() {
        let ty = PhpType::parse("42");
        assert_eq!(ty, PhpType::literal_int("42"));
    }

    #[test]
    fn literal_string() {
        let ty = PhpType::parse("'foo'");
        assert_eq!(ty, PhpType::literal_string_raw("'foo'"));
    }

    // ─── extract_value_type tests ───────────────────────────────────────────

    #[test]
    fn extract_value_type_array_slice() {
        let ty = PhpType::parse("User[]");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(*val, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_array_slice_scalar_skipped() {
        let ty = PhpType::parse("int[]");
        assert!(ty.extract_value_type(true).is_none());
    }

    #[test]
    fn extract_value_type_array_slice_scalar_not_skipped() {
        let ty = PhpType::parse("int[]");
        let val = ty.extract_value_type(false).unwrap();
        assert_eq!(*val, PhpType::int());
    }

    #[test]
    fn extract_value_type_list() {
        let ty = PhpType::parse("list<User>");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(*val, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_array_two_params() {
        let ty = PhpType::parse("array<int, User>");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(*val, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_collection() {
        let ty = PhpType::parse("Collection<int, User>");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(*val, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_generator() {
        // Generator<TKey, TValue, TSend, TReturn> — value is 2nd param
        let ty = PhpType::parse("Generator<int, User, mixed, void>");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(*val, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_generator_single_param() {
        // Single-param Generator<User> — treated as value type
        let ty = PhpType::parse("Generator<User>");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(*val, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_nullable() {
        let ty = PhpType::parse("?list<User>");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(*val, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_scalar_returns_none() {
        let ty = PhpType::int();
        assert!(ty.extract_value_type(true).is_none());
    }

    #[test]
    fn extract_value_type_plain_class_returns_none() {
        let ty = PhpType::Named("User".to_owned());
        assert!(ty.extract_value_type(true).is_none());
    }

    #[test]
    fn extract_value_type_union_with_generic_array() {
        // User|array<User> — the array member carries the element type.
        let ty = PhpType::parse("User|array<User>");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(val, &PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_union_with_array_slice() {
        // string|User[] — the array-slice member carries the element type.
        let ty = PhpType::parse("string|User[]");
        let val = ty.extract_value_type(true).unwrap();
        assert_eq!(val, &PhpType::Named("User".to_owned()));
    }

    #[test]
    fn extract_value_type_union_no_array_member() {
        // string|int — no array-like member, so no value type.
        let ty = PhpType::parse("string|int");
        assert!(ty.extract_value_type(false).is_none());
    }

    #[test]
    fn extract_value_type_union_skips_scalar_element() {
        // User|array<int> — with skip_scalar=true, the int element is skipped.
        let ty = PhpType::parse("User|array<int>");
        assert!(ty.extract_value_type(true).is_none());
    }

    #[test]
    fn extract_value_type_union_includes_scalar_element() {
        // User|array<int> — with skip_scalar=false, the int element is returned.
        let ty = PhpType::parse("User|array<int>");
        let val = ty.extract_value_type(false).unwrap();
        assert_eq!(val, &PhpType::Named("int".to_owned()));
    }

    // ─── extract_key_type tests ─────────────────────────────────────────────

    #[test]
    fn extract_key_type_two_params() {
        let ty = PhpType::parse("array<string, User>");
        let key = ty.extract_key_type(false).unwrap();
        assert_eq!(*key, PhpType::string());
    }

    #[test]
    fn extract_key_type_scalar_skipped() {
        let ty = PhpType::parse("array<int, User>");
        assert!(ty.extract_key_type(true).is_none());
    }

    #[test]
    fn extract_key_type_single_param_returns_none() {
        let ty = PhpType::parse("list<User>");
        assert!(ty.extract_key_type(false).is_none());
    }

    #[test]
    fn extract_key_type_slice_returns_none() {
        let ty = PhpType::parse("User[]");
        assert!(ty.extract_key_type(false).is_none());
    }

    #[test]
    fn extract_key_type_class_key() {
        let ty = PhpType::parse("array<Request, Response>");
        let key = ty.extract_key_type(true).unwrap();
        assert_eq!(*key, PhpType::Named("Request".to_owned()));
    }

    #[test]
    fn extract_key_type_union_with_keyed_array() {
        // User|array<string, User> — the array member carries the key type.
        let ty = PhpType::parse("User|array<string, User>");
        let key = ty.extract_key_type(false).unwrap();
        assert_eq!(*key, PhpType::Named("string".to_owned()));
    }

    #[test]
    fn extract_key_type_union_no_keyed_member() {
        // string|int — no array-like member, so no key type.
        let ty = PhpType::parse("string|int");
        assert!(ty.extract_key_type(false).is_none());
    }

    // ─── non_null_type tests ────────────────────────────────────────────────

    #[test]
    fn non_null_type_nullable() {
        let ty = PhpType::parse("?User");
        let non_null = ty.non_null_type().unwrap();
        assert_eq!(non_null, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn non_null_type_union_with_null() {
        let ty = PhpType::parse("User|null");
        let non_null = ty.non_null_type().unwrap();
        assert_eq!(non_null, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn non_null_type_union_multiple_non_null() {
        let ty = PhpType::parse("User|Admin|null");
        let non_null = ty.non_null_type().unwrap();
        match non_null {
            PhpType::Union(members) => {
                assert_eq!(members.len(), 2);
                assert_eq!(members[0], PhpType::Named("User".to_owned()));
                assert_eq!(members[1], PhpType::Named("Admin".to_owned()));
            }
            other => panic!("Expected Union, got {other:?}"),
        }
    }

    #[test]
    fn non_null_type_no_null() {
        let ty = PhpType::Named("User".to_owned());
        assert!(ty.non_null_type().is_none());
    }

    #[test]
    fn non_null_type_bare_null() {
        let ty = PhpType::null();
        assert!(ty.non_null_type().is_none());
    }

    // ─── all_members_scalar tests ───────────────────────────────────────────

    #[test]
    fn all_members_scalar_int() {
        assert!(PhpType::int().all_members_scalar());
    }

    #[test]
    fn all_members_scalar_string_or_null() {
        assert!(PhpType::parse("string|null").all_members_scalar());
    }

    #[test]
    fn all_members_scalar_nullable_int() {
        assert!(PhpType::parse("?int").all_members_scalar());
    }

    #[test]
    fn all_members_scalar_class() {
        assert!(!PhpType::Named("User".to_owned()).all_members_scalar());
    }

    #[test]
    fn all_members_scalar_class_or_null() {
        assert!(!PhpType::parse("User|null").all_members_scalar());
    }

    #[test]
    fn all_members_scalar_mixed_union() {
        assert!(!PhpType::parse("int|User").all_members_scalar());
    }

    // ─── intersection_members tests ─────────────────────────────────────────

    #[test]
    fn intersection_members_of_intersection() {
        let ty = PhpType::parse("Countable&Traversable");
        let members = ty.intersection_members();
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn intersection_members_of_non_intersection() {
        let ty = PhpType::Named("User".to_owned());
        let members = ty.intersection_members();
        assert_eq!(members.len(), 1);
        assert_eq!(*members[0], PhpType::Named("User".to_owned()));
    }

    // ─── resolve_names tests ────────────────────────────────────────────────

    #[test]
    fn resolve_names_simple_class() {
        let ty = PhpType::Named("User".to_owned());
        let resolved = ty.resolve_names(&|name| format!("App\\Models\\{}", name));
        assert_eq!(resolved.to_string(), "App\\Models\\User");
    }

    #[test]
    fn resolve_names_scalar_untouched() {
        let ty = PhpType::int();
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "int");
    }

    #[test]
    fn resolve_names_union() {
        let ty = PhpType::parse("User|null");
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "App\\User|null");
    }

    #[test]
    fn resolve_names_generic() {
        let ty = PhpType::parse("Collection<int, User>");
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "App\\Collection<int, App\\User>");
    }

    #[test]
    fn resolve_names_nullable() {
        let ty = PhpType::parse("?User");
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "?App\\User");
    }

    #[test]
    fn resolve_names_array_shape() {
        let ty = PhpType::parse("array{name: string, user: User}");
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "array{name: string, user: App\\User}");
    }

    #[test]
    fn resolve_names_callable() {
        let ty = PhpType::parse("callable(User): Response");
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "callable(App\\User): App\\Response");
    }

    #[test]
    fn resolve_names_keyword_types_untouched() {
        // All of these should pass through without calling the resolver.
        for kw in &[
            "self",
            "static",
            "parent",
            "$this",
            "mixed",
            "void",
            "never",
            "class-string",
            "key-of",
            "value-of",
            "callable",
            "iterable",
            "positive-int",
            "non-empty-string",
            "array-key",
        ] {
            let ty = PhpType::Named(kw.to_string());
            let resolved = ty.resolve_names(&|name| panic!("should not resolve {}", name));
            assert_eq!(resolved.to_string(), *kw);
        }
    }

    #[test]
    fn resolve_names_class_string_inner() {
        let ty = PhpType::parse("class-string<User>");
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "class-string<App\\User>");
    }

    #[test]
    fn resolve_names_intersection() {
        let ty = PhpType::parse("Countable&Traversable");
        let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
        assert_eq!(resolved.to_string(), "App\\Countable&App\\Traversable");
    }

    // ─── shorten tests ──────────────────────────────────────────────────────

    #[test]
    fn shorten_plain_class() {
        let ty = PhpType::Named("App\\Models\\User".to_owned());
        assert_eq!(ty.shorten().to_string(), "User");
    }

    #[test]
    fn shorten_already_short() {
        let ty = PhpType::Named("User".to_owned());
        assert_eq!(ty.shorten().to_string(), "User");
    }

    #[test]
    fn shorten_scalar() {
        let ty = PhpType::string();
        assert_eq!(ty.shorten().to_string(), "string");
    }

    #[test]
    fn shorten_union() {
        let ty = PhpType::parse("App\\Models\\User|null");
        assert_eq!(ty.shorten().to_string(), "User|null");
    }

    #[test]
    fn shorten_generic() {
        let ty = PhpType::parse("array<int, App\\Models\\User>");
        assert_eq!(ty.shorten().to_string(), "array<int, User>");
    }

    #[test]
    fn shorten_nullable() {
        let ty = PhpType::parse("?App\\Models\\User");
        assert_eq!(ty.shorten().to_string(), "?User");
    }

    #[test]
    fn shorten_callable() {
        let ty = PhpType::parse("callable(App\\Models\\User): App\\Http\\Response");
        assert_eq!(ty.shorten().to_string(), "callable(User): Response");
    }

    #[test]
    fn shorten_class_string() {
        let ty = PhpType::parse("class-string<App\\Models\\User>");
        assert_eq!(ty.shorten().to_string(), "class-string<User>");
    }

    #[test]
    fn shorten_intersection() {
        let ty = PhpType::parse("App\\Contracts\\Countable&App\\Contracts\\Traversable");
        assert_eq!(ty.shorten().to_string(), "Countable&Traversable");
    }

    #[test]
    fn shorten_array_shape() {
        let ty = PhpType::parse("array{name: string, user: App\\Models\\User}");
        assert_eq!(ty.shorten().to_string(), "array{name: string, user: User}");
    }

    // ─── is_scalar tests ────────────────────────────────────────────────────

    #[test]
    fn is_scalar_keywords() {
        assert!(PhpType::int().is_scalar());
        assert!(PhpType::string().is_scalar());
        assert!(PhpType::bool().is_scalar());
        assert!(PhpType::float().is_scalar());
        assert!(!PhpType::mixed().is_scalar());
        assert!(PhpType::void().is_scalar());
        assert!(PhpType::null().is_scalar());
        assert!(PhpType::array().is_scalar());
        assert!(PhpType::callable().is_scalar());
        assert!(PhpType::iterable().is_scalar());
    }

    #[test]
    fn is_scalar_class_is_not() {
        assert!(!PhpType::Named("User".to_owned()).is_scalar());
        assert!(!PhpType::Named("App\\Models\\User".to_owned()).is_scalar());
    }

    #[test]
    fn is_scalar_generic_array() {
        assert!(PhpType::parse("array<int, string>").is_scalar());
    }

    #[test]
    fn is_scalar_generic_class() {
        assert!(!PhpType::parse("Collection<int, User>").is_scalar());
    }

    #[test]
    fn is_scalar_nullable_scalar() {
        assert!(PhpType::parse("?int").is_scalar());
    }

    #[test]
    fn is_scalar_nullable_class() {
        assert!(!PhpType::parse("?User").is_scalar());
    }

    // ─── is_array_like tests ────────────────────────────────────────────────

    #[test]
    fn is_array_like_named() {
        assert!(PhpType::array().is_array_like());
        assert!(PhpType::Named("list".to_owned()).is_array_like());
        assert!(PhpType::iterable().is_array_like());
        assert!(PhpType::Named("non-empty-array".to_owned()).is_array_like());
        assert!(PhpType::Named("non-empty-list".to_owned()).is_array_like());
    }

    #[test]
    fn is_array_like_generic() {
        assert!(PhpType::parse("array<int, string>").is_array_like());
        assert!(PhpType::parse("list<User>").is_array_like());
        assert!(PhpType::parse("non-empty-array<string, int>").is_array_like());
    }

    #[test]
    fn is_array_like_slice() {
        assert!(PhpType::parse("User[]").is_array_like());
        assert!(PhpType::parse("int[]").is_array_like());
    }

    #[test]
    fn is_array_like_shape() {
        assert!(PhpType::parse("array{name: string}").is_array_like());
    }

    #[test]
    fn is_array_like_nullable() {
        assert!(PhpType::parse("?array").is_array_like());
        assert!(PhpType::parse("?list<User>").is_array_like());
    }

    #[test]
    fn is_array_like_non_array() {
        assert!(!PhpType::string().is_array_like());
        assert!(!PhpType::int().is_array_like());
        assert!(!PhpType::Named("User".to_owned()).is_array_like());
        assert!(!PhpType::null().is_array_like());
        assert!(!PhpType::parse("Collection<int, User>").is_array_like());
    }

    // ─── base_name tests ────────────────────────────────────────────────────

    #[test]
    fn base_name_simple_class() {
        assert_eq!(
            PhpType::Named("App\\Models\\User".to_owned()).base_name(),
            Some("App\\Models\\User")
        );
    }

    #[test]
    fn base_name_strips_leading_backslash() {
        assert_eq!(
            PhpType::Named("\\App\\Models\\User".to_owned()).base_name(),
            Some("App\\Models\\User")
        );
    }

    #[test]
    fn base_name_generic_strips_leading_backslash() {
        assert_eq!(
            PhpType::Generic(
                "\\Collection".to_owned(),
                vec![PhpType::Named("User".to_owned())]
            )
            .base_name(),
            Some("Collection")
        );
    }

    #[test]
    fn base_name_nullable_strips_leading_backslash() {
        assert_eq!(
            PhpType::Nullable(Box::new(PhpType::Named("\\User".to_owned()))).base_name(),
            Some("User")
        );
    }

    #[test]
    fn base_name_generic_class() {
        assert_eq!(
            PhpType::parse("Collection<int, User>").base_name(),
            Some("Collection")
        );
    }

    #[test]
    fn base_name_scalar_returns_none() {
        assert_eq!(PhpType::int().base_name(), None);
    }

    #[test]
    fn base_name_nullable_class() {
        assert_eq!(PhpType::parse("?User").base_name(), Some("User"));
    }

    #[test]
    fn base_name_union_returns_none() {
        assert_eq!(PhpType::parse("User|null").base_name(), None);
    }

    // ─── union_members tests ────────────────────────────────────────────────

    #[test]
    fn union_members_of_union() {
        let ty = PhpType::parse("int|string|null");
        let members = ty.union_members();
        assert_eq!(members.len(), 3);
    }

    #[test]
    fn union_members_of_non_union() {
        let ty = PhpType::Named("User".to_owned());
        let members = ty.union_members();
        assert_eq!(members.len(), 1);
        assert_eq!(*members[0], PhpType::Named("User".to_owned()));
    }

    // ─── equivalent tests ───────────────────────────────────────────────────

    #[test]
    fn equivalent_identical() {
        let a = PhpType::Named("User".to_owned());
        let b = PhpType::Named("User".to_owned());
        assert!(a.equivalent(&b));
    }

    #[test]
    fn equivalent_fqn_vs_short() {
        let a = PhpType::Named("App\\Models\\User".to_owned());
        let b = PhpType::Named("User".to_owned());
        assert!(a.equivalent(&b));
    }

    #[test]
    fn equivalent_nullable() {
        let a = PhpType::parse("?App\\Models\\User");
        let b = PhpType::parse("?User");
        assert!(a.equivalent(&b));
    }

    #[test]
    fn equivalent_union_reordered() {
        let a = PhpType::parse("App\\Models\\User|null");
        let b = PhpType::parse("null|User");
        assert!(a.equivalent(&b));
    }

    #[test]
    fn equivalent_generic() {
        let a = PhpType::parse("array<int, App\\Models\\User>");
        let b = PhpType::parse("array<int, User>");
        assert!(a.equivalent(&b));
    }

    #[test]
    fn equivalent_nullable_vs_union_with_null() {
        // `?string` is semantically identical to `string|null`
        let a = PhpType::parse("?string");
        let b = PhpType::parse("string|null");
        assert!(a.equivalent(&b));
        assert!(b.equivalent(&a));
    }

    #[test]
    fn equivalent_nullable_vs_null_first_union() {
        // `?callable` is semantically identical to `null|callable`
        let a = PhpType::parse("?callable");
        let b = PhpType::parse("null|callable");
        assert!(a.equivalent(&b));
    }

    #[test]
    fn equivalent_nullable_vs_three_member_union_not_equal() {
        // `?Foo` is NOT equivalent to `Foo|Bar|null` (different arity)
        let a = PhpType::parse("?Foo");
        let b = PhpType::parse("Foo|Bar|null");
        assert!(!a.equivalent(&b));
    }

    #[test]
    fn equivalent_different_types() {
        let a = PhpType::Named("User".to_owned());
        let b = PhpType::Named("Post".to_owned());
        assert!(!a.equivalent(&b));
    }

    // ── replace_self ────────────────────────────────────────────

    #[test]
    fn replace_self_named() {
        let ty = PhpType::parse("self");
        assert_eq!(ty.replace_self("App\\User").to_string(), "App\\User");
    }

    #[test]
    fn replace_self_static() {
        let ty = PhpType::parse("static");
        assert_eq!(ty.replace_self("App\\User").to_string(), "App\\User");
    }

    #[test]
    fn replace_self_this() {
        let ty = PhpType::parse("$this");
        assert_eq!(ty.replace_self("App\\User").to_string(), "App\\User");
    }

    #[test]
    fn replace_self_in_union() {
        let ty = PhpType::parse("self|null");
        let replaced = ty.replace_self("App\\User");
        assert_eq!(replaced.to_string(), "App\\User|null");
    }

    #[test]
    fn replace_self_in_generic() {
        let ty = PhpType::parse("Collection<int, static>");
        let replaced = ty.replace_self("App\\User");
        assert_eq!(replaced.to_string(), "Collection<int, App\\User>");
    }

    #[test]
    fn replace_self_no_keywords_unchanged() {
        let ty = PhpType::parse("string");
        assert_eq!(ty.replace_self("App\\User").to_string(), "string");
    }

    #[test]
    fn replace_self_nullable() {
        let ty = PhpType::parse("?self");
        assert_eq!(ty.replace_self("App\\User").to_string(), "?App\\User");
    }

    #[test]
    fn replace_self_class_name_unchanged() {
        let ty = PhpType::parse("Collection<int, User>");
        assert_eq!(
            ty.replace_self("App\\Post").to_string(),
            "Collection<int, User>"
        );
    }

    #[test]
    fn replace_self_intersection() {
        let ty = PhpType::parse("self&JsonSerializable");
        let replaced = ty.replace_self("App\\User");
        assert_eq!(replaced.to_string(), "App\\User&JsonSerializable");
    }

    // ── extract_class_names (recursive) ─────────────────────────

    #[test]
    fn extract_class_names_simple() {
        let names = PhpType::parse("User").extract_class_names();
        assert_eq!(names, vec!["User"]);
    }

    #[test]
    fn extract_class_names_scalar() {
        let names = PhpType::parse("int").extract_class_names();
        assert!(names.is_empty());
    }

    #[test]
    fn extract_class_names_union() {
        let names = PhpType::parse("User|Admin|null").extract_class_names();
        assert_eq!(names, vec!["User", "Admin"]);
    }

    #[test]
    fn extract_class_names_generic_recurses() {
        let names = PhpType::parse("Collection<int, User>").extract_class_names();
        assert_eq!(names, vec!["Collection", "User"]);
    }

    #[test]
    fn extract_class_names_callable() {
        let names = PhpType::parse("Closure(User): Admin").extract_class_names();
        assert_eq!(names, vec!["User", "Admin"]);
    }

    #[test]
    fn extract_class_names_nullable() {
        let names = PhpType::parse("?User").extract_class_names();
        assert_eq!(names, vec!["User"]);
    }

    #[test]
    fn extract_class_names_no_duplicates() {
        let names = PhpType::parse("User|User").extract_class_names();
        assert_eq!(names, vec!["User"]);
    }

    // ── top_level_class_names ───────────────────────────────────

    #[test]
    fn top_level_class_names_simple() {
        let names = PhpType::parse("User").top_level_class_names();
        assert_eq!(names, vec!["User"]);
    }

    #[test]
    fn top_level_class_names_generic_base_only() {
        let names = PhpType::parse("Collection<int, User>").top_level_class_names();
        assert_eq!(names, vec!["Collection"]);
    }

    #[test]
    fn top_level_class_names_union() {
        let names = PhpType::parse("User|Admin").top_level_class_names();
        assert_eq!(names, vec!["User", "Admin"]);
    }

    #[test]
    fn top_level_class_names_nullable() {
        let names = PhpType::parse("?User").top_level_class_names();
        assert_eq!(names, vec!["User"]);
    }

    #[test]
    fn top_level_class_names_union_with_null() {
        let names = PhpType::parse("User|null").top_level_class_names();
        assert_eq!(names, vec!["User"]);
    }

    #[test]
    fn top_level_class_names_scalar_excluded() {
        let names = PhpType::parse("string|int").top_level_class_names();
        assert!(names.is_empty());
    }

    #[test]
    fn top_level_class_names_mixed_union() {
        let names = PhpType::parse("string|User|int|Admin|null").top_level_class_names();
        assert_eq!(names, vec!["User", "Admin"]);
    }

    #[test]
    fn top_level_class_names_array_of_class() {
        let names = PhpType::parse("User[]").top_level_class_names();
        assert_eq!(names, vec!["User"]);
    }

    #[test]
    fn top_level_class_names_array_shape_excluded() {
        let names = PhpType::parse("array{name: string}").top_level_class_names();
        assert!(names.is_empty());
    }

    #[test]
    fn top_level_class_names_intersection() {
        let names = PhpType::parse("User&JsonSerializable").top_level_class_names();
        assert_eq!(names, vec!["User", "JsonSerializable"]);
    }

    #[test]
    fn top_level_class_names_fqn() {
        let names = PhpType::parse("\\App\\Models\\User").top_level_class_names();
        assert_eq!(names, vec!["\\App\\Models\\User"]);
    }

    // ── substitute ──────────────────────────────────────────────────

    fn make_subs(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, PhpType> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), PhpType::parse(v)))
            .collect()
    }

    #[test]
    fn substitute_named_match() {
        let ty = PhpType::parse("TValue");
        let result = ty.substitute(&make_subs(&[("TValue", "User")]));
        assert_eq!(result.to_string(), "User");
    }

    #[test]
    fn substitute_named_no_match() {
        let ty = PhpType::parse("SomeClass");
        let result = ty.substitute(&make_subs(&[("TValue", "User")]));
        assert_eq!(result.to_string(), "SomeClass");
    }

    #[test]
    fn substitute_generic() {
        let ty = PhpType::parse("Collection<TKey, TValue>");
        let subs = make_subs(&[("TKey", "int"), ("TValue", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "Collection<int, User>");
    }

    #[test]
    fn substitute_generic_base_is_template() {
        let ty = PhpType::parse("TContainer<int>");
        let subs = make_subs(&[("TContainer", "Collection")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "Collection<int>");
    }

    #[test]
    fn substitute_union() {
        let ty = PhpType::parse("TValue|null");
        let subs = make_subs(&[("TValue", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "User|null");
    }

    #[test]
    fn substitute_intersection() {
        let ty = PhpType::parse("TFirst&TSecond");
        let subs = make_subs(&[("TFirst", "Countable"), ("TSecond", "Iterator")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "Countable&Iterator");
    }

    #[test]
    fn substitute_nullable() {
        let ty = PhpType::parse("?TValue");
        let subs = make_subs(&[("TValue", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "?User");
    }

    #[test]
    fn substitute_array_shorthand() {
        let ty = PhpType::parse("TValue[]");
        let subs = make_subs(&[("TValue", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "array<User>");
    }

    #[test]
    fn substitute_array_shape() {
        let ty = PhpType::parse("array{name: TValue, age: int}");
        let subs = make_subs(&[("TValue", "string")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "array{name: string, age: int}");
    }

    #[test]
    fn substitute_object_shape() {
        let ty = PhpType::parse("object{item: TValue}");
        let subs = make_subs(&[("TValue", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "object{item: User}");
    }

    #[test]
    fn substitute_callable() {
        let ty = PhpType::parse("Closure(TParam): TReturn");
        let subs = make_subs(&[("TParam", "int"), ("TReturn", "string")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "Closure(int): string");
    }

    #[test]
    fn substitute_callable_no_return() {
        let ty = PhpType::parse("callable(TParam)");
        let subs = make_subs(&[("TParam", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "callable(User)");
    }

    #[test]
    fn substitute_class_string() {
        let ty = PhpType::parse("class-string<T>");
        let subs = make_subs(&[("T", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "class-string<User>");
    }

    #[test]
    fn substitute_key_of() {
        let ty = PhpType::parse("key-of<T>");
        let subs = make_subs(&[("T", "array<string, int>")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "string");
    }

    #[test]
    fn substitute_value_of() {
        let ty = PhpType::parse("value-of<T>");
        let subs = make_subs(&[("T", "array<string, User>")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "User");
    }

    #[test]
    fn substitute_nested_generic() {
        let ty = PhpType::parse("Collection<int, Promise<TValue>>");
        let subs = make_subs(&[("TValue", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "Collection<int, Promise<User>>");
    }

    #[test]
    fn substitute_empty_subs_unchanged() {
        let ty = PhpType::parse("Collection<int, User>");
        let subs: std::collections::HashMap<String, PhpType> = std::collections::HashMap::new();
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "Collection<int, User>");
    }

    #[test]
    fn substitute_scalar_unchanged() {
        let ty = PhpType::parse("int");
        let subs = make_subs(&[("TValue", "User")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "int");
    }

    #[test]
    fn substitute_literal_unchanged() {
        let ty = PhpType::parse("42");
        let subs = make_subs(&[("42", "User")]);
        // Literal nodes are not substituted (only Named nodes are).
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "42");
    }

    #[test]
    fn substitute_conditional() {
        let ty = PhpType::parse("($x is T ? TTrue : TFalse)");
        let subs = make_subs(&[("T", "string"), ("TTrue", "User"), ("TFalse", "null")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "$x is string ? User : null");
    }

    #[test]
    fn substitute_complex_real_world() {
        // Simulates resolving `Generator<TKey, TValue, TSend, TReturn>`
        // with concrete types.
        let ty = PhpType::parse("Generator<TKey, TValue, TSend, TReturn>");
        let subs = make_subs(&[
            ("TKey", "int"),
            ("TValue", "User"),
            ("TSend", "mixed"),
            ("TReturn", "void"),
        ]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "Generator<int, User, mixed, void>");
    }

    #[test]
    fn substitute_replacement_is_complex_type() {
        // When a template param is replaced with a union type.
        let ty = PhpType::parse("array<int, TValue>");
        let subs = make_subs(&[("TValue", "string|int")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "array<int, string|int>");
    }

    #[test]
    fn substitute_union_flattens_nested() {
        // When a union member is replaced with another union.
        let ty = PhpType::parse("TValue|null");
        let subs = make_subs(&[("TValue", "string|int")]);
        let result = ty.substitute(&subs);
        // Should flatten to a single union, not `(string|int)|null`.
        match &result {
            PhpType::Union(members) => assert_eq!(members.len(), 3),
            other => panic!("expected Union, got: {other}"),
        }
    }

    #[test]
    fn substitute_index_access() {
        let ty = PhpType::parse("T[K]");
        let subs = make_subs(&[("T", "array<string, int>"), ("K", "string")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "int");
    }

    #[test]
    fn substitute_interface_string() {
        let ty = PhpType::parse("interface-string<T>");
        let subs = make_subs(&[("T", "Countable")]);
        let result = ty.substitute(&subs);
        assert_eq!(result.to_string(), "interface-string<Countable>");
    }

    // ─── callable_param_types tests ─────────────────────────────────────────

    #[test]
    fn callable_param_types_on_callable() {
        let ty = PhpType::parse("callable(int, string): bool");
        let params = ty.callable_param_types().unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].type_hint, PhpType::int());
        assert_eq!(params[1].type_hint, PhpType::string());
    }

    #[test]
    fn callable_param_types_nullable_callable() {
        let ty = PhpType::parse("?Closure(int): void");
        let params = ty.callable_param_types().unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].type_hint, PhpType::int());
    }

    #[test]
    fn callable_param_types_union_with_callable() {
        let ty = PhpType::parse("Closure(string, int): void|null");
        let params = ty.callable_param_types().unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].type_hint, PhpType::string());
        assert_eq!(params[1].type_hint, PhpType::int());
    }

    #[test]
    fn callable_param_types_non_callable() {
        let ty = PhpType::int();
        assert!(ty.callable_param_types().is_none());
    }

    // ─── callable_return_type tests ─────────────────────────────────────────

    #[test]
    fn callable_return_type_with_return() {
        let ty = PhpType::parse("callable(int): User");
        let ret = ty.callable_return_type().unwrap();
        assert_eq!(*ret, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn callable_return_type_without_return() {
        let ty = PhpType::Callable {
            kind: "callable".to_owned(),
            params: vec![],
            return_type: None,
        };
        assert!(ty.callable_return_type().is_none());
    }

    #[test]
    fn callable_return_type_nullable_callable() {
        let ty = PhpType::parse("?Closure(string): User");
        let ret = ty.callable_return_type().unwrap();
        assert_eq!(*ret, PhpType::Named("User".to_owned()));
    }

    #[test]
    fn callable_return_type_union_with_callable() {
        let ty = PhpType::parse("Closure(int): Response|null");
        let ret = ty.callable_return_type().unwrap();
        assert_eq!(*ret, PhpType::Named("Response".to_owned()));
    }

    #[test]
    fn callable_return_type_non_callable() {
        let ty = PhpType::string();
        assert!(ty.callable_return_type().is_none());
    }

    // ─── generator_send_type tests ──────────────────────────────────────────

    #[test]
    fn generator_send_type_full_generator() {
        let ty = PhpType::parse("Generator<int, string, MyClass, void>");
        let send = ty.generator_send_type(false).unwrap();
        assert_eq!(*send, PhpType::Named("MyClass".to_owned()));
    }

    #[test]
    fn generator_send_type_skip_scalar_false_returns_scalar() {
        let ty = PhpType::parse("Generator<int, string, int, void>");
        let send = ty.generator_send_type(false).unwrap();
        assert_eq!(*send, PhpType::int());
    }

    #[test]
    fn generator_send_type_skip_scalar_true_skips_scalar() {
        let ty = PhpType::parse("Generator<int, string, int, void>");
        assert!(ty.generator_send_type(true).is_none());
    }

    #[test]
    fn generator_send_type_skip_scalar_true_keeps_class() {
        let ty = PhpType::parse("Generator<int, string, MyClass, void>");
        let send = ty.generator_send_type(true).unwrap();
        assert_eq!(*send, PhpType::Named("MyClass".to_owned()));
    }

    #[test]
    fn generator_send_type_fewer_than_three_params() {
        let ty = PhpType::parse("Generator<int, string>");
        assert!(ty.generator_send_type(false).is_none());
    }

    #[test]
    fn generator_send_type_non_generator() {
        let ty = PhpType::Named("Collection".to_owned());
        assert!(ty.generator_send_type(false).is_none());
    }

    #[test]
    fn generator_send_type_nullable_generator() {
        let ty = PhpType::parse("?Generator<int, string, MyClass, void>");
        let send = ty.generator_send_type(false).unwrap();
        assert_eq!(*send, PhpType::Named("MyClass".to_owned()));
    }

    // ── Subtype checking tests ──────────────────────────────────────────

    mod subtype_tests {
        use super::*;

        // ── Reflexivity ─────────────────────────────────────────────────

        #[test]
        fn subtype_reflexive_named() {
            let t = PhpType::int();
            assert!(t.is_subtype_of(&t));
        }

        #[test]
        fn subtype_reflexive_generic() {
            let t = PhpType::parse("array<int, string>");
            assert!(t.is_subtype_of(&t));
        }

        // ── Never and mixed ─────────────────────────────────────────────

        #[test]
        fn never_is_subtype_of_everything() {
            let never = PhpType::never();
            assert!(never.is_subtype_of(&PhpType::int()));
            assert!(never.is_subtype_of(&PhpType::string()));
            assert!(never.is_subtype_of(&PhpType::mixed()));
            assert!(never.is_subtype_of(&PhpType::parse("array<int>")));
        }

        #[test]
        fn everything_is_subtype_of_mixed() {
            let mixed = PhpType::mixed();
            assert!(PhpType::int().is_subtype_of(&mixed));
            assert!(PhpType::string().is_subtype_of(&mixed));
            assert!(PhpType::parse("Foo").is_subtype_of(&mixed));
            assert!(PhpType::parse("array<int>").is_subtype_of(&mixed));
        }

        // ── Bool subtypes ───────────────────────────────────────────────

        #[test]
        fn true_is_subtype_of_bool() {
            assert!(PhpType::true_().is_subtype_of(&PhpType::bool()));
        }

        #[test]
        fn false_is_subtype_of_bool() {
            assert!(PhpType::false_().is_subtype_of(&PhpType::bool()));
        }

        #[test]
        fn bool_is_not_subtype_of_true() {
            assert!(!PhpType::bool().is_subtype_of(&PhpType::true_()));
        }

        // ── Int <: float ────────────────────────────────────────────────

        #[test]
        fn int_is_subtype_of_float() {
            assert!(PhpType::int().is_subtype_of(&PhpType::float()));
        }

        #[test]
        fn float_is_not_subtype_of_int() {
            assert!(!PhpType::float().is_subtype_of(&PhpType::int()));
        }

        // ── Scalar refinements ──────────────────────────────────────────

        #[test]
        fn positive_int_is_subtype_of_int() {
            assert!(PhpType::Named("positive-int".into()).is_subtype_of(&PhpType::int()));
        }

        #[test]
        fn non_empty_string_is_subtype_of_string() {
            assert!(PhpType::Named("non-empty-string".into()).is_subtype_of(&PhpType::string()));
        }

        #[test]
        fn class_string_is_subtype_of_string() {
            assert!(PhpType::Named("class-string".into()).is_subtype_of(&PhpType::string()));
        }

        #[test]
        fn list_is_subtype_of_array() {
            assert!(PhpType::Named("list".into()).is_subtype_of(&PhpType::array()));
        }

        #[test]
        fn non_empty_list_is_subtype_of_non_empty_array() {
            assert!(
                PhpType::Named("non-empty-list".into())
                    .is_subtype_of(&PhpType::Named("non-empty-array".into()))
            );
        }

        #[test]
        fn array_is_subtype_of_iterable() {
            assert!(PhpType::array().is_subtype_of(&PhpType::iterable()));
        }

        #[test]
        fn closure_is_subtype_of_callable() {
            assert!(PhpType::Named("Closure".into()).is_subtype_of(&PhpType::callable()));
        }

        #[test]
        fn fqn_closure_is_subtype_of_callable() {
            assert!(PhpType::Named("\\Closure".into()).is_subtype_of(&PhpType::callable()));
        }

        #[test]
        fn fqn_closure_is_subtype_of_callable_union_null() {
            let callable_or_null = PhpType::Union(vec![PhpType::callable(), PhpType::null()]);
            assert!(PhpType::Named("\\Closure".into()).is_subtype_of(&callable_or_null));
        }

        // ── Scalar / numeric / array-key supertypes ─────────────────────

        #[test]
        fn int_is_subtype_of_scalar() {
            assert!(PhpType::int().is_subtype_of(&PhpType::Named("scalar".into())));
        }

        #[test]
        fn string_is_subtype_of_array_key() {
            assert!(PhpType::string().is_subtype_of(&PhpType::Named("array-key".into())));
        }

        #[test]
        fn int_is_subtype_of_numeric() {
            assert!(PhpType::int().is_subtype_of(&PhpType::numeric()));
        }

        // ── Nullable / union subtyping ──────────────────────────────────

        #[test]
        fn null_is_subtype_of_nullable() {
            assert!(PhpType::null().is_subtype_of(&PhpType::parse("?string")));
        }

        #[test]
        fn string_is_subtype_of_nullable_string() {
            assert!(PhpType::string().is_subtype_of(&PhpType::parse("?string")));
        }

        #[test]
        fn nullable_is_not_subtype_of_non_nullable() {
            assert!(!PhpType::parse("?string").is_subtype_of(&PhpType::string()));
        }

        #[test]
        fn union_member_is_subtype_of_union() {
            assert!(PhpType::int().is_subtype_of(&PhpType::parse("int|string")));
        }

        #[test]
        fn union_is_subtype_when_all_members_are() {
            assert!(PhpType::parse("int|float").is_subtype_of(&PhpType::float()));
        }

        #[test]
        fn union_is_not_subtype_when_member_is_not() {
            assert!(!PhpType::parse("int|string").is_subtype_of(&PhpType::int()));
        }

        // ── Intersection subtyping ──────────────────────────────────────

        #[test]
        fn intersection_is_subtype_when_any_member_is() {
            // Foo & Bar <: Foo
            let inter = PhpType::Intersection(vec![
                PhpType::Named("Foo".into()),
                PhpType::Named("Bar".into()),
            ]);
            assert!(inter.is_subtype_of(&PhpType::Named("Foo".into())));
        }

        #[test]
        fn subtype_of_intersection_requires_all() {
            // Foo <: Foo & Bar — false (Foo is not necessarily a Bar)
            let inter = PhpType::Intersection(vec![
                PhpType::Named("Foo".into()),
                PhpType::Named("Bar".into()),
            ]);
            assert!(!PhpType::Named("Foo".into()).is_subtype_of(&inter));
        }

        // ── Array / generic subtyping ───────────────────────────────────

        #[test]
        fn array_slice_is_subtype_of_array() {
            assert!(PhpType::parse("string[]").is_subtype_of(&PhpType::array()));
        }

        #[test]
        fn array_slice_is_not_subtype_of_object() {
            assert!(!PhpType::parse("string[]").is_subtype_of(&PhpType::object()));
        }

        #[test]
        fn array_shape_is_subtype_of_array() {
            assert!(PhpType::parse("array{name: string}").is_subtype_of(&PhpType::array()));
        }

        #[test]
        fn array_shape_is_subtype_of_generic_array_string_mixed() {
            assert!(
                PhpType::parse("array{id: int, refunded_amount: string}")
                    .is_subtype_of(&PhpType::parse("array<string, mixed>"))
            );
        }

        #[test]
        fn array_shape_is_subtype_of_generic_array_string_scalar() {
            assert!(
                PhpType::parse("array{name: string, age: int}")
                    .is_subtype_of(&PhpType::parse("array<string, string|int>"))
            );
        }

        #[test]
        fn array_shape_not_subtype_of_generic_array_wrong_value() {
            // Shape has an int value but supertype requires string values.
            assert!(
                !PhpType::parse("array{name: string, age: int}")
                    .is_subtype_of(&PhpType::parse("array<string, string>"))
            );
        }

        #[test]
        fn array_shape_is_subtype_of_generic_array_single_param() {
            // array<mixed> — only value type checked.
            assert!(
                PhpType::parse("array{name: string, count: int}")
                    .is_subtype_of(&PhpType::parse("array<mixed>"))
            );
        }

        #[test]
        fn array_shape_is_subtype_of_array_slice() {
            // array{name: string, label: string} <: string[]
            assert!(
                PhpType::parse("array{name: string, label: string}")
                    .is_subtype_of(&PhpType::parse("string[]"))
            );
        }

        #[test]
        fn array_shape_not_subtype_of_array_slice_wrong_value() {
            assert!(
                !PhpType::parse("array{name: string, age: int}")
                    .is_subtype_of(&PhpType::parse("string[]"))
            );
        }

        #[test]
        fn array_shape_with_int_keys_subtype_of_array_int_mixed() {
            // Positional entries have int keys.
            assert!(
                PhpType::parse("array{string, int}")
                    .is_subtype_of(&PhpType::parse("array<int, mixed>"))
            );
        }

        #[test]
        fn generic_array_is_subtype_of_array() {
            assert!(PhpType::parse("array<int, string>").is_subtype_of(&PhpType::array()));
        }

        #[test]
        fn generic_array_covariance() {
            assert!(
                PhpType::parse("array<int, string>")
                    .is_subtype_of(&PhpType::parse("array<int, string>"))
            );
        }

        #[test]
        fn generic_list_is_subtype_of_generic_array() {
            assert!(PhpType::parse("list<string>").is_subtype_of(&PhpType::parse("array<string>")));
        }

        #[test]
        fn array_slice_covariance() {
            // int[] <: int[] — reflexive
            assert!(PhpType::parse("int[]").is_subtype_of(&PhpType::parse("int[]")));
        }

        // ── class-string subtyping ──────────────────────────────────────

        #[test]
        fn class_string_generic_is_subtype_of_bare_class_string() {
            assert!(
                PhpType::parse("class-string<User>").is_subtype_of(&PhpType::parse("class-string"))
            );
        }

        #[test]
        fn class_string_generic_is_subtype_of_string() {
            assert!(PhpType::parse("class-string<User>").is_subtype_of(&PhpType::string()));
        }

        // ── Callable subtyping ──────────────────────────────────────────

        #[test]
        fn callable_is_subtype_of_named_callable() {
            assert!(PhpType::parse("callable(int): string").is_subtype_of(&PhpType::callable()));
        }

        #[test]
        fn callable_covariant_return() {
            // callable(): int <: callable(): float (int <: float)
            assert!(
                PhpType::parse("callable(): int")
                    .is_subtype_of(&PhpType::parse("callable(): float"))
            );
        }

        // ── Literal subtyping ───────────────────────────────────────────

        #[test]
        fn literal_int_is_subtype_of_int() {
            assert!(PhpType::literal_int("42").is_subtype_of(&PhpType::int()));
        }

        #[test]
        fn literal_string_is_subtype_of_string() {
            assert!(PhpType::literal_string_raw("'hello'").is_subtype_of(&PhpType::string()));
        }

        #[test]
        fn literal_int_is_subtype_of_float() {
            assert!(PhpType::literal_int("42").is_subtype_of(&PhpType::float()));
        }

        #[test]
        fn literal_numeric_string_is_subtype_of_numeric_string() {
            assert!(
                PhpType::literal_string_raw("'0.00'")
                    .is_subtype_of(&PhpType::parse("numeric-string"))
            );
        }

        #[test]
        fn literal_integer_string_is_subtype_of_numeric_string() {
            assert!(
                PhpType::literal_string_raw("'42'")
                    .is_subtype_of(&PhpType::parse("numeric-string"))
            );
        }

        #[test]
        fn literal_non_numeric_string_is_not_subtype_of_numeric_string() {
            assert!(
                !PhpType::literal_string_raw("'hello'")
                    .is_subtype_of(&PhpType::parse("numeric-string"))
            );
        }

        #[test]
        fn literal_empty_string_is_not_subtype_of_non_empty_string() {
            assert!(
                !PhpType::literal_string_raw("''")
                    .is_subtype_of(&PhpType::parse("non-empty-string"))
            );
        }

        #[test]
        fn literal_non_empty_string_is_subtype_of_non_empty_string() {
            assert!(
                PhpType::literal_string_raw("'foo'")
                    .is_subtype_of(&PhpType::parse("non-empty-string"))
            );
        }

        #[test]
        fn literal_string_is_subtype_of_truthy_string() {
            assert!(
                PhpType::literal_string_raw("'foo'")
                    .is_subtype_of(&PhpType::parse("truthy-string"))
            );
        }

        #[test]
        fn literal_zero_string_is_not_subtype_of_truthy_string() {
            assert!(
                !PhpType::literal_string_raw("'0'").is_subtype_of(&PhpType::parse("truthy-string"))
            );
        }

        #[test]
        fn literal_empty_string_is_not_subtype_of_truthy_string() {
            assert!(
                !PhpType::literal_string_raw("''").is_subtype_of(&PhpType::parse("truthy-string"))
            );
        }

        #[test]
        fn literal_negative_numeric_string_is_subtype_of_numeric_string() {
            assert!(
                PhpType::literal_string_raw("'-3.14'")
                    .is_subtype_of(&PhpType::parse("numeric-string"))
            );
        }

        #[test]
        fn literal_source_syntax_string_is_not_subtype_of_numeric_string() {
            // A runtime string is numeric per PHP's is_numeric, which does
            // not accept underscores or hex/binary/octal prefixes even
            // though they are valid in PHP source literals.
            for raw in ["'1_000'", "'0xFF'", "'0b101'", "'0o17'"] {
                assert!(
                    !PhpType::literal_string_raw(raw)
                        .is_subtype_of(&PhpType::parse("numeric-string")),
                    "{raw} should not be a numeric-string"
                );
            }
        }

        // ── IntRange subtyping ──────────────────────────────────────────

        #[test]
        fn int_range_is_subtype_of_int() {
            assert!(PhpType::IntRange("0".into(), "100".into()).is_subtype_of(&PhpType::int()));
        }

        #[test]
        fn integer_literal_is_subtype_of_int_range() {
            assert!(
                PhpType::literal_int("10000")
                    .is_subtype_of(&PhpType::IntRange("0".into(), "max".into(),))
            );
            assert!(
                PhpType::literal_int("1")
                    .is_subtype_of(&PhpType::IntRange("1".into(), "59".into(),))
            );
            assert!(
                !PhpType::literal_int("0")
                    .is_subtype_of(&PhpType::IntRange("1".into(), "59".into(),))
            );
        }

        #[test]
        fn float_literal_is_not_subtype_of_int_range() {
            assert!(
                !PhpType::literal_float("1.0")
                    .is_subtype_of(&PhpType::IntRange("0".into(), "max".into(),))
            );
        }

        #[test]
        fn php_integer_literal_syntaxes_compare_by_value() {
            assert!(PhpType::literal_int("0x10").is_subtype_of(&PhpType::literal_int("16")));
            assert!(PhpType::literal_int("0b1010").is_subtype_of(&PhpType::literal_int("10")));
            assert!(PhpType::literal_int("0o10").is_subtype_of(&PhpType::literal_int("8")));
            assert!(PhpType::literal_int("1_000").is_subtype_of(&PhpType::literal_int("1000")));
        }

        #[test]
        fn php_float_literal_syntaxes_compare_by_value() {
            assert!(PhpType::literal_float("1.0").is_subtype_of(&PhpType::literal_float("1.00")));
            assert!(PhpType::literal_float("1e3").is_subtype_of(&PhpType::literal_float("1000.0")));
            assert!(
                PhpType::literal_float("1_2.5e2").is_subtype_of(&PhpType::literal_float("1250.0"))
            );
        }

        // ── Unrelated types ─────────────────────────────────────────────

        #[test]
        fn string_is_not_subtype_of_int() {
            assert!(!PhpType::string().is_subtype_of(&PhpType::int()));
        }

        #[test]
        fn unrelated_classes_are_not_subtypes() {
            assert!(!PhpType::Named("Cat".into()).is_subtype_of(&PhpType::Named("Dog".into())));
        }

        // ── Aliases ─────────────────────────────────────────────────────

        #[test]
        fn integer_alias_subtype_of_int() {
            assert!(PhpType::Named("integer".into()).is_subtype_of(&PhpType::int()));
        }

        #[test]
        fn boolean_alias_subtype_of_bool() {
            assert!(PhpType::Named("boolean".into()).is_subtype_of(&PhpType::bool()));
        }

        // ── object shape <: object ──────────────────────────────────────

        #[test]
        fn object_shape_is_subtype_of_object() {
            assert!(PhpType::parse("object{name: string}").is_subtype_of(&PhpType::object()));
        }
    }

    // ── Simplification tests ────────────────────────────────────────────

    mod simplification_tests {
        use super::*;

        #[test]
        fn dedup_union() {
            let t = PhpType::Union(vec![PhpType::string(), PhpType::string()]);
            assert_eq!(t.simplified().to_string(), "string");
        }

        #[test]
        fn true_false_becomes_bool() {
            let t = PhpType::Union(vec![PhpType::true_(), PhpType::false_()]);
            assert_eq!(t.simplified().to_string(), "bool");
        }

        #[test]
        fn true_false_with_extra_member() {
            let t = PhpType::Union(vec![PhpType::true_(), PhpType::false_(), PhpType::null()]);
            let s = t.simplified();
            let display = s.to_string();
            assert!(display.contains("bool"), "should contain bool: {display}");
            assert!(display.contains("null"), "should contain null: {display}");
            assert!(
                !display.contains("true"),
                "should not contain true: {display}"
            );
            assert!(
                !display.contains("false"),
                "should not contain false: {display}"
            );
        }

        #[test]
        fn mixed_absorbs_union() {
            let t = PhpType::Union(vec![PhpType::mixed(), PhpType::string(), PhpType::int()]);
            assert_eq!(t.simplified().to_string(), "mixed");
        }

        #[test]
        fn scalar_refinement_absorbed() {
            let t = PhpType::Union(vec![PhpType::Named("positive-int".into()), PhpType::int()]);
            assert_eq!(t.simplified().to_string(), "int");
        }

        #[test]
        fn non_empty_string_absorbed_by_string() {
            let t = PhpType::Union(vec![
                PhpType::Named("non-empty-string".into()),
                PhpType::string(),
            ]);
            assert_eq!(t.simplified().to_string(), "string");
        }

        #[test]
        fn list_absorbed_by_array() {
            let t = PhpType::Union(vec![PhpType::Named("list".into()), PhpType::array()]);
            assert_eq!(t.simplified().to_string(), "array");
        }

        #[test]
        fn single_member_union_unwrapped() {
            let t = PhpType::Union(vec![PhpType::int()]);
            assert_eq!(t.simplified(), PhpType::int());
        }

        #[test]
        fn single_member_intersection_unwrapped() {
            let t = PhpType::Intersection(vec![PhpType::Named("Foo".into())]);
            assert_eq!(t.simplified(), PhpType::Named("Foo".into()));
        }

        #[test]
        fn nullable_never_becomes_null() {
            let t = PhpType::Nullable(Box::new(PhpType::never()));
            assert_eq!(t.simplified(), PhpType::null());
        }

        #[test]
        fn nullable_null_becomes_null() {
            let t = PhpType::Nullable(Box::new(PhpType::null()));
            assert_eq!(t.simplified(), PhpType::null());
        }

        #[test]
        fn nullable_mixed_becomes_mixed() {
            let t = PhpType::Nullable(Box::new(PhpType::mixed()));
            assert_eq!(t.simplified(), PhpType::mixed());
        }

        #[test]
        fn nested_union_flattened() {
            let t = PhpType::Union(vec![
                PhpType::Union(vec![
                    PhpType::Named("Foo".into()),
                    PhpType::Named("Bar".into()),
                ]),
                PhpType::Named("Baz".into()),
            ]);
            let s = t.simplified();
            if let PhpType::Union(members) = &s {
                assert_eq!(members.len(), 3);
            } else {
                panic!("Expected Union, got {s:?}");
            }
        }

        #[test]
        fn nested_intersection_flattened() {
            let t = PhpType::Intersection(vec![
                PhpType::Intersection(vec![
                    PhpType::Named("Foo".into()),
                    PhpType::Named("Bar".into()),
                ]),
                PhpType::Named("Baz".into()),
            ]);
            let s = t.simplified();
            if let PhpType::Intersection(members) = &s {
                assert_eq!(members.len(), 3);
            } else {
                panic!("Expected Intersection, got {s:?}");
            }
        }

        #[test]
        fn intersection_with_never_collapses() {
            let t = PhpType::Intersection(vec![PhpType::Named("Foo".into()), PhpType::never()]);
            assert_eq!(t.simplified(), PhpType::never());
        }

        #[test]
        fn generic_args_simplified() {
            let t = PhpType::Generic(
                "array".into(),
                vec![PhpType::Union(vec![PhpType::true_(), PhpType::false_()])],
            );
            let s = t.simplified();
            assert_eq!(s.to_string(), "array<bool>");
        }

        #[test]
        fn dedup_case_insensitive() {
            let t = PhpType::Union(vec![PhpType::Named("String".into()), PhpType::string()]);
            // Should deduplicate — only one remains.
            let s = t.simplified();
            assert!(
                !matches!(s, PhpType::Union(_)),
                "should be unwrapped: {s:?}"
            );
        }

        #[test]
        fn closure_subtype_of_callable() {
            // Ensure Closure <: callable works in subtype check (case-insensitive).
            assert!(PhpType::Named("closure".into()).is_subtype_of(&PhpType::callable()));
        }
    }

    // ── Intersection distribution tests ─────────────────────────────────

    mod distribute_tests {
        use super::*;

        #[test]
        fn distribute_simple() {
            // (A|B) & C → (A&C) | (B&C)
            let t = PhpType::Intersection(vec![
                PhpType::Union(vec![PhpType::Named("A".into()), PhpType::Named("B".into())]),
                PhpType::Named("C".into()),
            ]);
            let d = t.distribute_intersection();
            if let PhpType::Union(members) = &d {
                assert_eq!(members.len(), 2);
            } else {
                panic!("Expected Union, got {d:?}");
            }
        }

        #[test]
        fn distribute_no_union_unchanged() {
            let t = PhpType::Intersection(vec![
                PhpType::Named("Foo".into()),
                PhpType::Named("Bar".into()),
            ]);
            let d = t.distribute_intersection();
            assert_eq!(d, t);
        }

        #[test]
        fn distribute_non_intersection_unchanged() {
            let t = PhpType::Named("Foo".into());
            let d = t.distribute_intersection();
            assert_eq!(d, t);
        }

        #[test]
        fn distribute_two_unions() {
            // (A|B) & (C|D) → (A&C) | (A&D) | (B&C) | (B&D)
            let t = PhpType::Intersection(vec![
                PhpType::Union(vec![PhpType::Named("A".into()), PhpType::Named("B".into())]),
                PhpType::Union(vec![PhpType::Named("C".into()), PhpType::Named("D".into())]),
            ]);
            let d = t.distribute_intersection();
            if let PhpType::Union(members) = &d {
                assert_eq!(members.len(), 4, "Expected 4 members, got {d}");
            } else {
                panic!("Expected Union, got {d:?}");
            }
        }

        #[test]
        fn distribute_with_simplification() {
            // (A|A) & B → after distribution and simplification → A & B
            let t = PhpType::Intersection(vec![
                PhpType::Union(vec![PhpType::Named("A".into()), PhpType::Named("A".into())]),
                PhpType::Named("B".into()),
            ]);
            let d = t.distribute_intersection();
            // The union (A|A) deduplicates to A, so the result should be A&B.
            assert!(
                matches!(d, PhpType::Intersection(_)),
                "Expected Intersection, got {d:?}"
            );
        }
    }

    // ── Predicate tests ─────────────────────────────────────────────────

    mod predicate_tests {
        use super::*;

        // ── is_bool ─────────────────────────────────────────────────

        #[test]
        fn is_bool_true_for_bool() {
            assert!(PhpType::bool().is_bool());
        }

        #[test]
        fn is_bool_true_for_boolean() {
            assert!(PhpType::Named("boolean".into()).is_bool());
        }

        #[test]
        fn is_bool_case_insensitive() {
            assert!(PhpType::Named("Bool".into()).is_bool());
            assert!(PhpType::Named("BOOLEAN".into()).is_bool());
        }

        #[test]
        fn is_bool_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::bool())).is_bool());
        }

        #[test]
        fn is_bool_false_for_int() {
            assert!(!PhpType::int().is_bool());
        }

        #[test]
        fn is_bool_false_for_true() {
            assert!(!PhpType::true_().is_bool());
        }

        // ── is_true ────────────────────────────────────────────────

        #[test]
        fn is_true_true_for_true() {
            assert!(PhpType::true_().is_true());
        }

        #[test]
        fn is_true_case_insensitive() {
            assert!(PhpType::Named("True".into()).is_true());
            assert!(PhpType::Named("TRUE".into()).is_true());
        }

        #[test]
        fn is_true_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::true_())).is_true());
        }

        #[test]
        fn is_true_false_for_false() {
            assert!(!PhpType::false_().is_true());
        }

        #[test]
        fn is_true_false_for_bool() {
            assert!(!PhpType::bool().is_true());
        }

        // ── is_false ───────────────────────────────────────────────

        #[test]
        fn is_false_true_for_false() {
            assert!(PhpType::false_().is_false());
        }

        #[test]
        fn is_false_case_insensitive() {
            assert!(PhpType::Named("False".into()).is_false());
            assert!(PhpType::Named("FALSE".into()).is_false());
        }

        #[test]
        fn is_false_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::false_())).is_false());
        }

        #[test]
        fn is_false_false_for_true() {
            assert!(!PhpType::true_().is_false());
        }

        #[test]
        fn is_false_false_for_bool() {
            assert!(!PhpType::bool().is_false());
        }

        // ── is_int ─────────────────────────────────────────────────

        #[test]
        fn is_int_true_for_int() {
            assert!(PhpType::int().is_int());
        }

        #[test]
        fn is_int_true_for_integer() {
            assert!(PhpType::Named("integer".into()).is_int());
        }

        #[test]
        fn is_int_case_insensitive() {
            assert!(PhpType::Named("Int".into()).is_int());
            assert!(PhpType::Named("INTEGER".into()).is_int());
        }

        #[test]
        fn is_int_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::int())).is_int());
        }

        #[test]
        fn is_int_false_for_float() {
            assert!(!PhpType::float().is_int());
        }

        // ── is_string_type ─────────────────────────────────────────

        #[test]
        fn is_string_type_true_for_string() {
            assert!(PhpType::string().is_string_type());
        }

        #[test]
        fn is_string_type_case_insensitive() {
            assert!(PhpType::Named("String".into()).is_string_type());
            assert!(PhpType::Named("STRING".into()).is_string_type());
        }

        #[test]
        fn is_string_type_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::string())).is_string_type());
        }

        #[test]
        fn is_string_type_false_for_int() {
            assert!(!PhpType::int().is_string_type());
        }

        #[test]
        fn is_string_type_false_for_class_string() {
            assert!(!PhpType::ClassString(None).is_string_type());
        }

        // ── is_float ───────────────────────────────────────────────

        #[test]
        fn is_float_true_for_float() {
            assert!(PhpType::float().is_float());
        }

        #[test]
        fn is_float_true_for_double() {
            assert!(PhpType::Named("double".into()).is_float());
        }

        #[test]
        fn is_float_case_insensitive() {
            assert!(PhpType::Named("Float".into()).is_float());
            assert!(PhpType::Named("DOUBLE".into()).is_float());
        }

        #[test]
        fn is_float_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::float())).is_float());
        }

        #[test]
        fn is_float_false_for_int() {
            assert!(!PhpType::int().is_float());
        }

        // ── is_object ──────────────────────────────────────────────

        #[test]
        fn is_object_true_for_object() {
            assert!(PhpType::object().is_object());
        }

        #[test]
        fn is_object_case_insensitive() {
            assert!(PhpType::Named("Object".into()).is_object());
            assert!(PhpType::Named("OBJECT".into()).is_object());
        }

        #[test]
        fn is_object_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::object())).is_object());
        }

        #[test]
        fn is_object_false_for_class() {
            assert!(!PhpType::Named("User".into()).is_object());
        }

        #[test]
        fn is_object_false_for_object_shape() {
            assert!(!PhpType::ObjectShape(vec![]).is_object());
        }

        // ── is_callable ────────────────────────────────────────────

        #[test]
        fn is_callable_true_for_callable() {
            assert!(PhpType::callable().is_callable());
        }

        #[test]
        fn is_callable_case_insensitive() {
            assert!(PhpType::Named("Callable".into()).is_callable());
            assert!(PhpType::Named("CALLABLE".into()).is_callable());
        }

        #[test]
        fn is_callable_true_for_closure() {
            assert!(PhpType::Named("Closure".into()).is_callable());
            assert!(PhpType::Named("closure".into()).is_callable());
        }

        #[test]
        fn is_callable_true_for_callable_variant() {
            let t = PhpType::Callable {
                kind: "callable".into(),
                params: vec![],
                return_type: None,
            };
            assert!(t.is_callable());
        }

        #[test]
        fn is_callable_true_for_closure_variant() {
            let t = PhpType::Callable {
                kind: "Closure".into(),
                params: vec![],
                return_type: Some(Box::new(PhpType::void())),
            };
            assert!(t.is_callable());
        }

        #[test]
        fn is_callable_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::callable())).is_callable());
            assert!(PhpType::Nullable(Box::new(PhpType::Named("Closure".into()))).is_callable());
        }

        #[test]
        fn is_callable_false_for_string() {
            assert!(!PhpType::string().is_callable());
        }

        // ── is_self_like ───────────────────────────────────────────

        #[test]
        fn is_self_like_true_for_self() {
            assert!(PhpType::self_().is_self_like());
        }

        #[test]
        fn is_self_like_true_for_static() {
            assert!(PhpType::static_().is_self_like());
        }

        #[test]
        fn is_self_like_true_for_this() {
            assert!(PhpType::Named("$this".into()).is_self_like());
        }

        #[test]
        fn is_self_like_true_for_parent() {
            assert!(PhpType::parent_().is_self_like());
        }

        #[test]
        fn is_self_like_case_insensitive() {
            assert!(PhpType::Named("Self".into()).is_self_like());
            assert!(PhpType::Named("STATIC".into()).is_self_like());
            assert!(PhpType::Named("Parent".into()).is_self_like());
        }

        #[test]
        fn is_self_like_nullable() {
            assert!(PhpType::Nullable(Box::new(PhpType::static_())).is_self_like());
        }

        #[test]
        fn is_self_like_false_for_class() {
            assert!(!PhpType::Named("User".into()).is_self_like());
        }

        #[test]
        fn is_self_like_false_for_int() {
            assert!(!PhpType::int().is_self_like());
        }

        #[test]
        fn is_self_ref_true_for_self() {
            assert!(PhpType::self_().is_self_ref());
        }

        #[test]
        fn is_self_ref_true_for_static() {
            assert!(PhpType::static_().is_self_ref());
        }

        #[test]
        fn is_self_ref_true_for_this() {
            assert!(PhpType::Named("$this".to_string()).is_self_ref());
        }

        #[test]
        fn is_self_ref_false_for_parent() {
            assert!(!PhpType::parent_().is_self_ref());
        }

        #[test]
        fn is_self_ref_case_insensitive() {
            assert!(PhpType::Named("SELF".to_string()).is_self_ref());
            assert!(PhpType::Named("Static".to_string()).is_self_ref());
        }

        #[test]
        fn is_self_ref_not_nullable() {
            assert!(!PhpType::Nullable(Box::new(PhpType::static_())).is_self_ref());
        }

        #[test]
        fn is_self_ref_false_for_class() {
            assert!(!PhpType::Named("Foo".to_string()).is_self_ref());
        }

        // ── is_bool/is_true/is_false for non-matching types ────────

        #[test]
        fn predicates_false_for_union() {
            let u = PhpType::Union(vec![PhpType::int(), PhpType::string()]);
            assert!(!u.is_bool());
            assert!(!u.is_true());
            assert!(!u.is_false());
            assert!(!u.is_int());
            assert!(!u.is_string_type());
            assert!(!u.is_float());
            assert!(!u.is_object());
            assert!(!u.is_array_key());
            assert!(!u.is_callable());
            assert!(!u.is_self_like());
        }

        // ── is_array_key ───────────────────────────────────────────

        #[test]
        fn is_array_key_true_for_array_key() {
            assert!(PhpType::parse("array-key").is_array_key());
        }

        #[test]
        fn is_array_key_case_insensitive() {
            assert!(PhpType::parse("Array-Key").is_array_key());
            assert!(PhpType::parse("ARRAY-KEY").is_array_key());
        }

        #[test]
        fn is_array_key_nullable() {
            assert!(PhpType::parse("?array-key").is_array_key());
        }

        #[test]
        fn is_array_key_false_for_int() {
            assert!(!PhpType::parse("int").is_array_key());
        }

        #[test]
        fn is_array_key_false_for_string() {
            assert!(!PhpType::parse("string").is_array_key());
        }

        // ── is_iterable ────────────────────────────────────────────

        #[test]
        fn is_iterable_true_for_iterable() {
            assert!(PhpType::parse("iterable").is_iterable());
        }

        #[test]
        fn is_iterable_case_insensitive() {
            assert!(PhpType::parse("Iterable").is_iterable());
            assert!(PhpType::parse("ITERABLE").is_iterable());
        }

        #[test]
        fn is_iterable_nullable() {
            assert!(PhpType::parse("?iterable").is_iterable());
        }

        #[test]
        fn is_iterable_false_for_array() {
            assert!(!PhpType::parse("array").is_iterable());
        }

        #[test]
        fn is_iterable_false_for_iterator() {
            assert!(!PhpType::parse("Iterator").is_iterable());
        }

        // ── is_closure ─────────────────────────────────────────────

        #[test]
        fn is_closure_true_for_closure() {
            assert!(PhpType::parse("Closure").is_closure());
        }

        #[test]
        fn is_closure_true_for_fqn_closure() {
            assert!(PhpType::parse("\\Closure").is_closure());
        }

        #[test]
        fn is_closure_case_insensitive() {
            assert!(PhpType::parse("closure").is_closure());
            assert!(PhpType::parse("CLOSURE").is_closure());
        }

        #[test]
        fn is_closure_nullable() {
            assert!(PhpType::parse("?Closure").is_closure());
        }

        #[test]
        fn is_closure_callable_variant() {
            let ty = PhpType::Callable {
                kind: "Closure".to_string(),
                params: vec![],
                return_type: Some(Box::new(PhpType::void())),
            };
            assert!(ty.is_closure());
        }

        #[test]
        fn is_closure_false_for_callable() {
            assert!(!PhpType::parse("callable").is_closure());
        }

        #[test]
        fn is_closure_false_for_string() {
            assert!(!PhpType::parse("string").is_closure());
        }

        // ── is_resource ────────────────────────────────────────────

        #[test]
        fn is_resource_true_for_resource() {
            assert!(PhpType::parse("resource").is_resource());
        }

        #[test]
        fn is_resource_case_insensitive() {
            assert!(PhpType::parse("Resource").is_resource());
            assert!(PhpType::parse("RESOURCE").is_resource());
        }

        #[test]
        fn is_resource_nullable() {
            assert!(PhpType::parse("?resource").is_resource());
        }

        #[test]
        fn is_resource_false_for_string() {
            assert!(!PhpType::parse("string").is_resource());
        }

        // ── is_empty_sentinel ──────────────────────────────────────

        #[test]
        fn is_empty_sentinel_true() {
            assert!(PhpType::Named("__empty".to_string()).is_empty_sentinel());
            assert!(PhpType::empty_sentinel().is_empty_sentinel());
        }

        #[test]
        fn is_named_case_sensitive() {
            assert!(PhpType::Named("TModel".to_string()).is_named("TModel"));
            assert!(!PhpType::Named("TModel".to_string()).is_named("tmodel"));
            assert!(PhpType::int().is_named("int"));
            assert!(!PhpType::int().is_named("INT"));
        }

        #[test]
        fn is_named_false_for_non_named() {
            assert!(!PhpType::Generic("list".to_string(), vec![PhpType::int()]).is_named("list"));
            assert!(
                !PhpType::Nullable(Box::new(PhpType::Named("Foo".to_string()))).is_named("Foo")
            );
        }

        #[test]
        fn is_named_ci_case_insensitive() {
            assert!(PhpType::Named("stdClass".to_string()).is_named_ci("stdclass"));
            assert!(PhpType::Named("stdclass".to_string()).is_named_ci("stdClass"));
            assert!(!PhpType::Named("Foo".to_string()).is_named_ci("Bar"));
        }

        #[test]
        fn list_constructor() {
            let ty = PhpType::list(PhpType::string());
            assert_eq!(ty.to_string(), "list<string>");
        }

        #[test]
        fn generic_array_constructor() {
            let ty = PhpType::generic_array(PhpType::string(), PhpType::int());
            assert_eq!(ty.to_string(), "array<string, int>");
        }

        #[test]
        fn generic_array_val_constructor() {
            let ty = PhpType::generic_array_val(PhpType::int());
            assert_eq!(ty.to_string(), "array<int>");
        }

        #[test]
        fn is_empty_sentinel_false_for_regular() {
            assert!(!PhpType::parse("string").is_empty_sentinel());
            assert!(!PhpType::parse("").is_empty_sentinel());
        }

        #[test]
        fn is_string_literal_single_quoted() {
            assert!(PhpType::literal_string_raw("'hello'").is_string_literal());
        }

        #[test]
        fn is_string_literal_double_quoted() {
            assert!(PhpType::literal_string_raw("\"world\"").is_string_literal());
        }

        #[test]
        fn is_string_literal_false_for_int() {
            assert!(!PhpType::literal_int("42").is_string_literal());
        }

        #[test]
        fn is_string_literal_false_for_named() {
            assert!(!PhpType::Named("string".to_owned()).is_string_literal());
        }

        #[test]
        fn is_int_literal_positive() {
            assert!(PhpType::literal_int("42").is_int_literal());
        }

        #[test]
        fn is_int_literal_negative() {
            assert!(PhpType::literal_int("-1").is_int_literal());
        }

        #[test]
        fn is_int_literal_false_for_string() {
            assert!(!PhpType::literal_string_raw("'hello'").is_int_literal());
        }

        #[test]
        fn is_int_literal_false_for_named() {
            assert!(!PhpType::Named("int".to_owned()).is_int_literal());
        }
    }

    // ─── Parenthesized type parsing ─────────────────────────────────────

    #[test]
    fn parse_grouped_union_array() {
        // `(string|int)[]` should parse as Array(Union(string, int))
        let ty = PhpType::parse("(string|int)[]");
        assert!(
            matches!(&ty, PhpType::Array(inner) if matches!(inner.as_ref(), PhpType::Union(_))),
            "expected Array(Union(...)), got: {:?}",
            ty
        );
    }

    #[test]
    fn parse_parenthesized_callable() {
        // `(callable(): string)` should parse and unwrap the parens
        let ty = PhpType::parse("(callable(): string)");
        assert!(
            matches!(&ty, PhpType::Callable { .. }),
            "expected Callable, got: {:?}",
            ty
        );
    }

    #[test]
    fn parse_callable_return_string() {
        let ty = PhpType::parse("callable():string");
        assert!(
            matches!(&ty, PhpType::Callable { .. }),
            "expected Callable, got: {:?}",
            ty
        );
    }
}
