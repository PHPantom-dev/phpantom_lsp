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

    /// Resolve relative class-reference keywords to concrete class names,
    /// walking the entire type tree (including array elements and generic
    /// arguments).
    ///
    /// `self`, `static`, and `$this` become `class_name`; `parent` becomes
    /// `parent_class` when it is `Some`.  Unlike [`resolve_names`], which
    /// treats these keywords as non-class types and leaves them untouched,
    /// this resolves them so a declared type can be compared against a
    /// resolved value type.
    ///
    /// [`resolve_names`]: PhpType::resolve_names
    pub fn resolve_self_refs(&self, class_name: &str, parent_class: Option<&str>) -> PhpType {
        // self / static / $this — case-insensitive, whole-tree walk.
        let replaced = self.replace_self(class_name);
        match parent_class {
            Some(parent) => {
                let subs = std::collections::HashMap::from([(
                    "parent".to_string(),
                    PhpType::Named(parent.to_string()),
                )]);
                replaced.substitute(&subs)
            }
            None => replaced,
        }
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

        // ── array-key normalisation ─────────────────────────────────
        // `array-key` is exactly `int|string`. Expanding it here lets a
        // subject typed `array-key` satisfy an `int|string` supertype
        // (the union-supertype check below only tries each member in
        // isolation, and `array-key` is a subtype of neither `int` nor
        // `string` alone). Reflexive `array-key <: array-key` is already
        // handled above, so this only fires against structural supertypes.
        if self.is_array_key() {
            let as_union = PhpType::Union(vec![PhpType::int(), PhpType::string()]);
            return as_union.is_subtype_of(supertype);
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

        // ── IntRange <: int / refined-int / IntRange ────────────────
        if let PhpType::IntRange(sub_min, sub_max) = self {
            match supertype {
                // IntRange <: int, numeric, scalar, array-key
                PhpType::Named(sup) => {
                    let sup_l = sup.to_ascii_lowercase();
                    if matches!(
                        sup_l.as_str(),
                        "int" | "integer" | "numeric" | "scalar" | "array-key"
                    ) {
                        return true;
                    }
                    // IntRange <: refined-int (e.g. int<0,max> <: non-negative-int)
                    if let Some((sup_min, sup_max)) = refined_int_to_range(&sup_l) {
                        return int_range_is_subrange(sub_min, sub_max, sup_min, sup_max);
                    }
                    // IntRange <: non-zero-int — the range must not contain 0.
                    // Either entirely positive (min >= 1) or entirely negative (max <= -1).
                    if sup_l == "non-zero-int" {
                        let lo = parse_range_bound(sub_min);
                        let hi = parse_range_bound(sub_max);
                        return lo >= 1 || hi <= -1;
                    }
                    return false;
                }
                // IntRange <: IntRange (e.g. int<1,100> <: int<0,max>)
                PhpType::IntRange(sup_min, sup_max) => {
                    return int_range_is_subrange(sub_min, sub_max, sup_min, sup_max);
                }
                _ => {}
            }
        }

        // ── refined-int <: IntRange ─────────────────────────────────
        // e.g. non-negative-int <: int<0,max>, positive-int <: int<0,max>
        if let PhpType::Named(sub) = self
            && let PhpType::IntRange(sup_min, sup_max) = supertype
        {
            let sub_l = sub.to_ascii_lowercase();
            if let Some((sub_min, sub_max)) = refined_int_to_range(&sub_l) {
                return int_range_is_subrange(sub_min, sub_max, sup_min, sup_max);
            }
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

    // `number` is a PHPDoc pseudo-type (int|float) only in its exact lowercase
    // spelling. A differently-cased bare `Number` (e.g. `BcMath\Number`) is a
    // real class; the identical-string case was handled above, and nominal
    // class relationships are resolved by the caller's hierarchy check, so
    // here it has no scalar sub/supertype relationship.
    if (sub_l == "number" && sub_raw != "number") || (sup_l == "number" && sup_raw != "number") {
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
        // ── refined-int cross-subtyping ─────────────────────────
        // e.g. positive-int <: non-negative-int (1..max ⊆ 0..max)
        "positive-int" | "negative-int" | "non-positive-int" | "non-negative-int"
            if refined_int_to_range(sub_n).is_some() && refined_int_to_range(sup_n).is_some() =>
        {
            let (sub_min, sub_max) = refined_int_to_range(sub_n).unwrap();
            let (sup_min, sup_max) = refined_int_to_range(sup_n).unwrap();
            int_range_is_subrange(sub_min, sub_max, sup_min, sup_max)
        }

        // ── non-zero-int supertype ──────────────────────────────
        // positive-int and negative-int are subtypes of non-zero-int.
        // non-negative-int and non-positive-int are NOT (they include 0).
        "non-zero-int" => matches!(sub_n, "positive-int" | "negative-int"),

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
            // A differently-cased bare `Number` (e.g. `BcMath\Number`) is a
            // real class, not the lowercase `number` pseudo-type; a scalar
            // literal is never a subtype of it.
            if sup_l == "number" && sup != "number" {
                return false;
            }
            // Integer literal → int (and its supertypes).
            if matches!(lit, LiteralValue::Int(_)) {
                if matches!(
                    sup_l.as_str(),
                    "int"
                        | "integer"
                        | "float"
                        | "double"
                        | "numeric"
                        | "number"
                        | "scalar"
                        | "array-key"
                ) {
                    return true;
                }
                // Named refined-int types: check the literal's value
                // against the refinement's constraint directly, rather
                // than falling through to a name comparison that can
                // never match a literal.
                if let Some((min, max)) = refined_int_to_range(&sup_l) {
                    return lit
                        .parse_i64()
                        .is_some_and(|value| int_literal_is_within_range(value, min, max));
                }
                if sup_l == "non-zero-int" {
                    return lit.parse_i64().is_some_and(|value| value != 0);
                }
                return false;
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

/// Convert a refined-int type name to its equivalent `(min, max)` range
/// bounds.  Returns `None` for non-refined types.
///
/// - `positive-int`     → `("1", "max")`
/// - `negative-int`     → `("min", "-1")`
/// - `non-negative-int` → `("0", "max")`
/// - `non-positive-int` → `("min", "0")`
fn refined_int_to_range(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "positive-int" => Some(("1", "max")),
        "negative-int" => Some(("min", "-1")),
        "non-negative-int" => Some(("0", "max")),
        "non-positive-int" => Some(("min", "0")),
        _ => None,
    }
}

/// Parse a range bound string into an `i64`, treating `"min"` as
/// `i64::MIN` and `"max"` as `i64::MAX`.
fn parse_range_bound(s: &str) -> i64 {
    match s.trim().to_ascii_lowercase().as_str() {
        "min" => i64::MIN,
        "max" => i64::MAX,
        v => v.parse::<i64>().unwrap_or(0),
    }
}

/// Check whether range `(sub_min, sub_max)` is fully contained within
/// `(sup_min, sup_max)`.  Both bounds are inclusive.
fn int_range_is_subrange(sub_min: &str, sub_max: &str, sup_min: &str, sup_max: &str) -> bool {
    let sub_lo = parse_range_bound(sub_min);
    let sub_hi = parse_range_bound(sub_max);
    let sup_lo = parse_range_bound(sup_min);
    let sup_hi = parse_range_bound(sup_max);
    sup_lo <= sub_lo && sub_hi <= sup_hi
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
            // ── PHPStan lenient-union wrapper ───────────────────────
            | "__benevolent"
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
    // `number` is a PHPDoc-only pseudo-type (int|float) and only in its exact
    // lowercase spelling. PHP has no native `number` type and allows a class
    // named `Number` (e.g. PHP 8.4's `BcMath\Number`), so any other casing is
    // a real class reference and must not be classified as a scalar.
    if name == "number" {
        return true;
    }
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
        | "array-key" | "scalar" | "numeric" => lower,
        // `number` is a pseudo-type only in lowercase; fall through so a
        // `Number` class keeps its casing and lowercase `number` stays as-is.
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
        "scalar" | "numeric" | "array-key" | "empty-scalar" | "non-empty-scalar" | "empty" => None,

        // `number` (int|float) has no single native equivalent, but only in
        // its lowercase pseudo-type spelling; a `Number` class passes through.
        "number" if name == "number" => None,

        // Anything else is a class name — pass it through.
        _ => Some(name),
    }
}

/// Convert a borrowed mago AST `Type` into an owned `PhpType`.
fn convert(ty: &cst::Type<'_>) -> PhpType {
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
fn convert_keyword_with_optional_generics(
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
fn flatten_union(ty: &cst::Type<'_>) -> Vec<PhpType> {
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
fn flatten_intersection(ty: &cst::Type<'_>) -> Vec<PhpType> {
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
#[path = "php_type_tests.rs"]
mod tests;
