//! Structured representation of PHP type expressions.
//!
//! This module provides [`PhpType`], an owned enum that represents PHP type
//! expressions as a tree. It is converted from the borrowed
//! `mago_type_syntax::cst::Type<'input>` AST and can be displayed back into a
//! canonical string form.
//!
//! # Design
//!
//! `mago_type_syntax::cst::Type` is `#[non_exhaustive]` with 69 variants and
//! borrows from input. `PhpType` is simpler: keyword types are collapsed into
//! `Named`, generic-parameterised references become `Generic`, and rarely-used
//! variants fall back to `Raw`.
//!
//! `PhpType::parse()` never fails. If the input cannot be parsed or mapped,
//! it returns `PhpType::Raw(input)`.

use std::fmt;

use mago_allocator::{Arena, LocalArena};
use mago_database::file::FileId;
use mago_span::{Position, Span};
use mago_type_syntax::cst;

use crate::atom::bytes_to_str;

mod display;
mod keywords;
mod normalize;
mod parse;
mod subtype;
mod transform;

pub(crate) use keywords::*;
pub(crate) use normalize::*;
pub(crate) use parse::*;
pub(crate) use subtype::*;

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

impl PhpType {
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

    /// Whether this type admits `null` as a value.
    ///
    /// Returns `true` for `null` itself, a `?T` nullable wrapper, `mixed`,
    /// and any union that contains a null member. Returns `false` for
    /// non-nullable types.
    pub fn accepts_null(&self) -> bool {
        match self {
            PhpType::Nullable(_) => true,
            PhpType::Union(members) => members.iter().any(|m| m.accepts_null()),
            PhpType::Named(s) => s.eq_ignore_ascii_case("null") || s.eq_ignore_ascii_case("mixed"),
            _ => false,
        }
    }

    /// Return a copy of this type that also admits `null`.
    ///
    /// Leaves the type unchanged when it already [`accepts_null`]. A bare
    /// type `T` becomes `?T`; a union `A|B` becomes `A|B|null`.
    ///
    /// [`accepts_null`]: PhpType::accepts_null
    #[must_use]
    pub fn or_null(self) -> PhpType {
        if self.accepts_null() {
            return self;
        }
        match self {
            PhpType::Union(mut members) => {
                members.push(PhpType::null());
                PhpType::Union(members)
            }
            other => PhpType::Nullable(Box::new(other)),
        }
    }

    /// Whether this type is a scalar/built-in type that does not refer
    /// to a user-defined class.
    ///
    /// Returns `true` when this type is exactly `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, PhpType::Named(s) if s.eq_ignore_ascii_case("null"))
    }

    /// Whether a [`PhpType::Conditional`] appears anywhere in this type tree.
    ///
    /// Used as a cheap guard before running the (cloning) nested-conditional
    /// evaluator over a method's resolved return type: conditionals embedded
    /// inside a generic wrapper (e.g. `Collection<($x is array ? … : …), …>`)
    /// need to be collapsed against the call arguments, but the vast majority
    /// of return types contain no conditional and can be left untouched.
    pub fn contains_conditional(&self) -> bool {
        match self {
            PhpType::Conditional { .. } => true,
            PhpType::Nullable(inner)
            | PhpType::Array(inner)
            | PhpType::ClassString(Some(inner))
            | PhpType::InterfaceString(Some(inner))
            | PhpType::KeyOf(inner)
            | PhpType::ValueOf(inner) => inner.contains_conditional(),
            PhpType::Union(members)
            | PhpType::Intersection(members)
            | PhpType::Generic(_, members) => members.iter().any(|m| m.contains_conditional()),
            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
                entries.iter().any(|e| e.value_type.contains_conditional())
            }
            PhpType::Callable {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| p.type_hint.contains_conditional())
                    || return_type
                        .as_ref()
                        .is_some_and(|r| r.contains_conditional())
            }
            PhpType::IndexAccess(base, index) => {
                base.contains_conditional() || index.contains_conditional()
            }
            _ => false,
        }
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
            // A shape is only a scalar leaf when every entry value is;
            // `array{price: Decimal}` yields the non-scalar `Decimal`
            // when indexed or iterated.
            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
                entries.iter().all(|e| e.value_type.is_scalar_leaf())
            }
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
            PhpType::Generic(_, args) if !args.is_empty() => {
                // Iterables follow the `<TKey, TValue>` convention: the value
                // is the *second* generic argument whenever two or more are
                // present. This covers `array<K, V>`, `Collection<K, V>`,
                // `Iterator<K, V>`, `Generator<TKey, TValue, TSend, TReturn>`,
                // and the SPL wrapper iterators
                // (`IteratorIterator`/`FilterIterator`/`AppendIterator`) that
                // append a third `TIterator` argument. With a single argument
                // (e.g. `list<User>`) that lone argument is the value.
                let value = if args.len() >= 2 {
                    Some(&args[1])
                } else {
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

    /// Return the element (value) type produced by iterating this type,
    /// as an owned type.
    ///
    /// Unlike [`extract_value_type`](Self::extract_value_type), this also
    /// handles array/object shapes: iterating a tuple-style `array{A, B}`
    /// yields `A|B`. For all other types it delegates to
    /// `extract_value_type(false)`, so generic collections (`list<User>`,
    /// `array<int, Order>`) behave exactly as before.
    pub fn iterable_element_type(&self) -> Option<PhpType> {
        match self {
            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
                let mut values: Vec<PhpType> = Vec::new();
                for entry in entries {
                    if !values.contains(&entry.value_type) {
                        values.push(entry.value_type.clone());
                    }
                }
                match values.len() {
                    0 => None,
                    1 => Some(values.into_iter().next().unwrap()),
                    _ => Some(PhpType::Union(values)),
                }
            }
            PhpType::Nullable(inner) => inner.iterable_element_type(),
            PhpType::Union(members) => members.iter().find_map(|m| m.iterable_element_type()),
            _ => self.extract_value_type(false).cloned(),
        }
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

    /// Join two array shapes into a single shape that covers both
    /// variants.
    ///
    /// This is the union of two shapes expressed as one shape:
    /// `array{a: int}` joined with `array{a: int, b: string}` is
    /// `array{a: int, b?: string}`.
    ///
    /// - A key present on both sides unions the two value types
    ///   (recursively joining nested shapes) and stays required unless
    ///   optional on either side.
    /// - A key present on only one side becomes optional — the other
    ///   variant does not guarantee it.
    ///
    /// Branch merging uses this to fold the shape a variable has after
    /// one branch with the shape it has after another.  Folding keeps
    /// the variable at a single tracked shape no matter how many
    /// branches write to it; accumulating one variant per branch
    /// instead makes every later merge compare all variants pairwise,
    /// which turns large procedural methods with hundreds of
    /// conditional writes into quadratic-and-worse walks.
    ///
    /// Handles `?array{…}` on either side (the join is nullable when
    /// either side is).  Returns `None` when either side is not an
    /// array shape or contains positional (unkeyed) entries — those are
    /// list-style shapes where a per-key join is not meaningful.
    pub fn join_shapes(&self, other: &PhpType) -> Option<PhpType> {
        match (self, other) {
            (PhpType::ArrayShape(a), PhpType::ArrayShape(b)) => {
                Some(PhpType::ArrayShape(Self::join_shape_entries(a, b)?))
            }
            (PhpType::Nullable(a), PhpType::Nullable(b)) => {
                Some(PhpType::Nullable(Box::new(a.join_shapes(b)?)))
            }
            (PhpType::Nullable(a), b @ PhpType::ArrayShape(_)) => {
                Some(PhpType::Nullable(Box::new(a.join_shapes(b)?)))
            }
            (a @ PhpType::ArrayShape(_), PhpType::Nullable(b)) => {
                Some(PhpType::Nullable(Box::new(a.join_shapes(b)?)))
            }
            _ => None,
        }
    }

    /// Join two keyed shape entry lists (see [`join_shapes`]).
    ///
    /// Keys from `a` keep their order; keys only in `b` follow in `b`'s
    /// order.  Returns `None` when either side has positional (unkeyed)
    /// entries.
    ///
    /// [`join_shapes`]: Self::join_shapes
    fn join_shape_entries(a: &[ShapeEntry], b: &[ShapeEntry]) -> Option<Vec<ShapeEntry>> {
        if a.iter().any(|e| e.key.is_none()) || b.iter().any(|e| e.key.is_none()) {
            return None;
        }
        let mut joined: Vec<ShapeEntry> = Vec::with_capacity(a.len().max(b.len()));
        for ea in a {
            match b.iter().find(|eb| eb.key == ea.key) {
                Some(eb) => joined.push(ShapeEntry {
                    key: ea.key.clone(),
                    value_type: Self::join_values(&ea.value_type, &eb.value_type),
                    optional: ea.optional || eb.optional,
                }),
                None => joined.push(ShapeEntry {
                    key: ea.key.clone(),
                    value_type: ea.value_type.clone(),
                    optional: true,
                }),
            }
        }
        for eb in b {
            if !a.iter().any(|ea| ea.key == eb.key) {
                joined.push(ShapeEntry {
                    key: eb.key.clone(),
                    value_type: eb.value_type.clone(),
                    optional: true,
                });
            }
        }
        Some(joined)
    }

    /// Union two value types for a joined shape key.
    ///
    /// Equivalent members are kept once and nested shape members are
    /// joined rather than accumulated, so repeated merges cannot grow
    /// the value type without bound.
    fn join_values(a: &PhpType, b: &PhpType) -> PhpType {
        if a.equivalent(b) {
            return a.clone();
        }
        let mut members: Vec<PhpType> = a.union_members().into_iter().cloned().collect();
        'incoming: for m in b.union_members() {
            for existing in members.iter_mut() {
                if existing.equivalent(m) {
                    continue 'incoming;
                }
                if let Some(joined) = existing.join_shapes(m) {
                    *existing = joined;
                    continue 'incoming;
                }
            }
            members.push(m.clone());
        }
        if members.len() == 1 {
            members.into_iter().next().unwrap()
        } else {
            PhpType::Union(members)
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
    /// Returns `true` only for `void` and `never` — the two types that
    /// genuinely carry no value. `mixed` is *informative*: it means "some
    /// value of unknown type", which downstream narrowing (`is_string`,
    /// `instanceof`, …) can still refine. Treating `mixed` as uninformative
    /// here would strip the type entirely and leave the variable untyped, so
    /// a conditional branch selecting `mixed` must flow `mixed` through.
    pub fn is_uninformative_return(&self) -> bool {
        self.is_void() || self.is_never()
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

#[cfg(test)]
#[path = "../php_type_tests.rs"]
mod tests;
