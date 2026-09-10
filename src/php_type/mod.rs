//! Structured representation of PHP type expressions.
//!
//! This module provides [`PhpType`], an owned enum that represents PHP type
//! expressions as a tree. It is converted from the borrowed
//! `mago_phpdoc_syntax::cst::type::Type<'arena>` AST and can be displayed back
//! into a canonical string form.
//!
//! # Design
//!
//! `mago_phpdoc_syntax::cst::type::Type` is `#[non_exhaustive]` with more than
//! eighty variants and borrows from the parse arena. `PhpType` is simpler:
//! keyword types are collapsed into `Named`, generic-parameterised references
//! become `Generic`, and rarely-used variants fall back to `Raw`.
//!
//! `PhpType::parse()` never fails. If the input cannot be parsed or mapped,
//! it returns `PhpType::raw(input)`.

use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_phpdoc_syntax::cst::r#type as cst;
use mago_span::{Position, Span};

use crate::atom::{Atom, atom, bytes_to_str};

mod display;
mod intern;
mod keywords;
mod normalize;
mod parse;
mod subtype;
mod transform;

#[cfg(any(test, feature = "mem-audit"))]
pub(crate) use intern::interned_len;
pub(crate) use keywords::*;
pub(crate) use normalize::*;
pub(crate) use parse::*;
pub(crate) use subtype::*;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A structured PHP type expression: a pointer-sized handle to a shared,
/// interned [`TypeKind`] node.
///
/// # Interning
///
/// Every value is built through the constructors below, which hash-cons the
/// node: two structurally equal types always share one allocation. That is
/// what makes a type occurrence cost 8 bytes rather than 24 plus a private
/// copy of its payload, on a workload where the same handful of forms
/// (`string`, `?Carbon`, `Collection<User>`) recur tens of thousands of
/// times. It also makes [`PartialEq`] a pointer comparison instead of a
/// structural walk, and [`Clone`] a refcount bump instead of a deep copy.
///
/// The tuple field is private for exactly that reason: a `PhpType` that did
/// not come from the interner would silently compare unequal to its own
/// structural twin. Match on [`kind`](PhpType::kind) to inspect the node.
///
/// See `php_type/intern.rs` for why the table is refcounted rather than
/// leaked the way [`Atom`] is.
#[derive(Clone)]
pub struct PhpType(Arc<TypeKind>);

impl PhpType {
    /// The interned node this handle points at, with any
    /// [`Benevolent`](TypeKind::Benevolent) or
    /// [`ListShape`](TypeKind::ListShape) marker seen through.
    ///
    /// Leniency is a note about where a type came from, and list-ness is an
    /// extra promise about a shape's keys; neither is a shape of its own, so
    /// neither may change how the type is matched on. Use
    /// [`raw_kind`](PhpType::raw_kind) to see the marker.
    ///
    /// The two markers never wrap each other — one only ever tags a union or
    /// nullable, the other only ever tags an array shape — so one hop is
    /// always enough to reach the node itself.
    #[inline]
    pub fn kind(&self) -> &TypeKind {
        match &*self.0 {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => &inner.0,
            kind => kind,
        }
    }

    /// The interned node this handle points at, marker and all.
    ///
    /// Only the type module itself should need this: the interner, the
    /// `Display` impl, and the transforms that have to carry the marker
    /// across a rewrite.
    #[inline]
    pub(crate) fn raw_kind(&self) -> &TypeKind {
        &self.0
    }

    /// Whether this type carries PHPStan's `__benevolent<>` leniency
    /// marker.
    #[inline]
    pub fn is_benevolent(&self) -> bool {
        matches!(&*self.0, TypeKind::Benevolent(_))
    }

    /// Tag `inner` as benevolent.
    ///
    /// A marker only means something on a union — it says "one of these
    /// branches is the failure branch nobody checks" — so anything else is
    /// returned untagged, and an already-tagged type is not tagged twice.
    /// That keeps the marker at the very top of the type, which is what
    /// lets [`kind`](PhpType::kind) see through it in one hop.
    pub fn benevolent(inner: PhpType) -> PhpType {
        if inner.is_benevolent() {
            return inner;
        }
        if !matches!(inner.kind(), TypeKind::Union(_) | TypeKind::Nullable(_)) {
            return inner;
        }
        TypeKind::Benevolent(inner).into()
    }

    /// This type without its leniency marker.
    #[inline]
    pub fn strip_benevolence(&self) -> PhpType {
        match &*self.0 {
            TypeKind::Benevolent(inner) => inner.clone(),
            _ => self.clone(),
        }
    }

    /// Address of the interned node, distinct for every distinct type as
    /// long as a handle to it is alive.
    ///
    /// Only meaningful as a hash of the identity `PartialEq` already
    /// compares; a cache keyed on it must hold the handle too, since
    /// dropping the last one frees the node and lets a later type reuse
    /// the address.
    #[inline]
    pub(crate) fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

impl Deref for PhpType {
    type Target = TypeKind;

    #[inline]
    fn deref(&self) -> &TypeKind {
        self.kind()
    }
}

/// Pointer equality. Sound because the interner guarantees that structurally
/// equal types share one allocation.
impl PartialEq for PhpType {
    #[inline]
    fn eq(&self, other: &PhpType) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PhpType {}

impl Hash for PhpType {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(Arc::as_ptr(&self.0) as usize);
    }
}

/// Renders as the node itself, so the handle is invisible in test output and
/// `{:?}` logging.
impl fmt::Debug for PhpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl From<TypeKind> for PhpType {
    #[inline]
    fn from(kind: TypeKind) -> PhpType {
        intern::intern(kind)
    }
}

/// The shape of a PHP type expression: the payload behind a [`PhpType`]
/// handle.
///
/// # Size
///
/// One node exists per *distinct* type rather than per occurrence, so this is
/// no longer duplicated millions of times. It is still kept to 24 bytes (two
/// pointers plus a discriminant) so that the interner's own table stays
/// small: the rare shapes that would otherwise dominate (`Generic`,
/// `Callable`, `Conditional`, `Literal`) hold a single `Box` to an
/// out-of-line payload struct, and the collection variants hold `Box<[T]>`
/// rather than `Vec<T>`. See the `php_type_size_is_bounded` test.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    /// A named type: keywords (`int`, `string`, `mixed`, `void`, …),
    /// class references (`Foo\Bar`), or special names (`self`, `static`,
    /// `parent`). Also used for PHPDoc variable references (`$this`).
    ///
    /// Interned as an [`Atom`]: `Named` values are drawn from a bounded set
    /// (class names, keywords, template parameters, `$this`), compared
    /// frequently, and cloned throughout the substitution hot path, so
    /// pointer-sized equality and free cloning pay off here. Literal scalar
    /// values live in [`TypeKind::Literal`], not here, so the interner is
    /// never fed unbounded free text.
    Named(Atom),

    /// Late-static-binding type with a known lower bound.
    ///
    /// Represents `static` or `$this` resolved in the context of a class:
    /// "the runtime class, which is at least `bound`."  Unlike
    /// [`Named`](TypeKind::Named) with the class FQN, this preserves the
    /// polymorphic semantics of `static` — a `StaticType("Base")` is a
    /// subtype of `Base` but is not the same as `Named("Base")` because it
    /// could be any subclass at runtime.
    ///
    /// Produced when `Named("static")` is resolved in a class context.
    /// `self` is NOT represented here — it resolves to
    /// `Named(declaring_class)` since it is invariant.
    StaticType(Atom),

    /// The `$this` type with a known lower bound.
    ///
    /// More specific than [`StaticType`](TypeKind::StaticType): represents
    /// the exact runtime instance, not just "this class or a subclass."
    /// `ThisType("Foo") <: StaticType("Foo") <: Named("Foo")`.
    ///
    /// Important for fluent interfaces (`@return $this`) where the return
    /// type must preserve the receiver's exact type through method chains.
    ThisType(Atom),

    /// Nullable type: `?T`.
    Nullable(PhpType),

    /// Union type: `T|U|V`. Always contains two or more members.
    Union(Box<[PhpType]>),

    /// Intersection type: `T&U`. Always contains two or more members.
    Intersection(Box<[PhpType]>),

    /// Generic (parameterised) type: `Collection<int, User>`, `array<string>`,
    /// `list<int>`, `non-empty-array<string>`, `iterable<K, V>`, etc.
    Generic(Box<GenericType>),

    /// The `T[]` slice syntax (sugar for `array<int, T>`).
    Array(PhpType),

    /// Array shape: `array{key: string, age?: int}`.
    ArrayShape(Box<[ShapeEntry]>),

    /// Object shape: `object{name: string}`.
    ObjectShape(Box<[ShapeEntry]>),

    /// Callable or Closure type with optional specification.
    /// `callable(int, string): bool`, `Closure(int): void`,
    /// `pure-callable(T): U`, `pure-Closure(T): U`.
    Callable(Box<CallableType>),

    /// Conditional return type: `$x is T ? U : V`.
    Conditional(Box<ConditionalType>),

    /// `class-string<T>` or bare `class-string`.
    ClassString(Option<PhpType>),

    /// `interface-string<T>` or bare `interface-string`.
    InterfaceString(Option<PhpType>),

    /// `key-of<T>`.
    KeyOf(PhpType),

    /// `value-of<T>`.
    ValueOf(PhpType),

    /// `int<min, max>` range type.
    ///
    /// The bounds are the literal source text of the range (`"min"`,
    /// `"max"`, or an integer), interned because they are drawn from a
    /// small set in practice.
    IntRange(Atom, Atom),

    /// Index access type: `T[K]`.
    IndexAccess(PhpType, PhpType),

    /// A literal scalar type with preserved kind and source text.
    Literal(Box<LiteralValue>),

    /// Fallback for anything we cannot parse or do not yet map.
    Raw(Box<str>),

    /// PHPStan's `__benevolent<T>` marker: the inner union, tagged as one
    /// whose failure branch is not worth enforcing.
    ///
    /// Deliberately invisible: [`PhpType::kind`] and the [`Deref`] impl
    /// both see straight through it to the inner node, so every existing
    /// `match ty.kind()` treats a benevolent `string|false` exactly as it
    /// treats a plain one.  Narrowing, subtyping, hover and completion are
    /// therefore unaffected, which is the point — the union is honest, and
    /// only the code that explicitly asks (via [`PhpType::is_benevolent`])
    /// gets to relax it.  [`PhpType::raw_kind`] is the way to see the
    /// marker itself.
    ///
    /// The cost of that invisibility is that a transform which rebuilds a
    /// type by matching on `kind()` drops the marker unless it matches on
    /// `raw_kind()` and carries it across.  The ones on the path from a
    /// stub return type to a diagnostic do (`resolve_names`, `substitute`,
    /// `simplified`, `shorten`, the `self`/`static` replacements, …); a
    /// transform that does not simply loses the leniency, which shows up
    /// as the `|false` diagnostic coming back, never as a wrong type.
    Benevolent(PhpType),

    /// `list{…}` / `non-empty-list{…}`: the inner [`ArrayShape`] with the
    /// extra promise that its keys are `0, 1, 2, …` in that order, which is
    /// what `array_is_list()` answers `true` for.
    ///
    /// Invisible in the same way [`Benevolent`](TypeKind::Benevolent) is:
    /// [`PhpType::kind`] and [`Deref`] see through to the shape, so every
    /// `match ty.kind()` keeps treating it as the `array{…}` it also is, and
    /// only [`PhpType::is_list_shape`] asks about the ordering promise.
    ///
    /// [`ArrayShape`]: TypeKind::ArrayShape
    ListShape(PhpType),
}

/// Payload of [`TypeKind::Generic`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericType {
    /// The base name: a class reference (`Collection`) or a parameterisable
    /// keyword (`array`, `list`, `iterable`, `class-string`).
    pub name: Atom,
    /// The type arguments between `<` and `>`.
    pub args: Vec<PhpType>,
}

/// Payload of [`TypeKind::Callable`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallableType {
    /// One of `callable`, `Closure`, `pure-callable`, `pure-Closure`.
    pub kind: Atom,
    /// Parameter types.
    pub params: Vec<CallableParam>,
    /// Optional return type.
    pub return_type: Option<PhpType>,
}

/// Payload of [`TypeKind::Conditional`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConditionalType {
    /// The subject (typically a variable like `$this`).
    pub param: Atom,
    /// Whether the condition is negated (`is not`).
    pub negated: bool,
    /// The condition type.
    pub condition: PhpType,
    /// The type when the condition is true.
    pub then_type: PhpType,
    /// The type when the condition is false.
    pub else_type: PhpType,
    /// Whether an undecidable condition answers with `else_type` rather
    /// than the union of both branches.
    ///
    /// A conditional someone wrote in a docblock describes two outcomes
    /// the call really can have, so an argument that settles neither
    /// leaves both on the table. A conditional we synthesised to patch a
    /// stub can be modelling something narrower: `pow()`'s `object`
    /// branch exists only for the operator-overloading extensions (GMP,
    /// BCMath), and an argument nobody gave a type is no reason to
    /// believe one of those is in play. Setting this on such a
    /// conditional keeps the ordinary answer instead of widening to a
    /// union with a branch the call almost certainly does not take.
    ///
    /// Never set from parsed PHPDoc — [`PhpType::conditional`] leaves it
    /// off, and only [`PhpType::conditional_defaulting_to_else`] turns
    /// it on.
    pub else_when_undecided: bool,
}

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

/// Whether PHP stores this string as an integer array key.
///
/// Only canonical decimal spellings are coerced: `"8"` and `"-8"` become
/// integer keys, while `"+8"`, `"08"`, decimal fractions, and values outside
/// the platform integer range remain strings.
pub(crate) fn is_decimal_int_array_key(content: &str) -> bool {
    !content.starts_with('+')
        && content
            .parse::<i64>()
            .is_ok_and(|parsed| parsed.to_string() == content)
}

/// The runtime array key each shape entry occupies, in order.
///
/// A positional entry takes the next free integer index, mirroring the
/// literal or the sequence of appends it was tracked from, and an explicit
/// integer key moves that cursor past itself. Returns `None` once an entry
/// that may be absent leaves a later positional entry's index in doubt,
/// since pairing two shapes up is only decidable when both sides' keys are.
pub(crate) fn runtime_shape_keys(entries: &[ShapeEntry]) -> Option<Vec<String>> {
    let mut next: i64 = 0;
    let mut shifted = false;
    let mut keys = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry.key.as_deref() {
            None => {
                if shifted {
                    return None;
                }
                keys.push(next.to_string());
                next = next.checked_add(1)?;
                shifted |= entry.optional;
            }
            Some(key) => {
                if let Ok(index) = key.parse::<i64>() {
                    next = next.max(index.checked_add(1)?);
                    shifted |= entry.optional;
                }
                keys.push(key.to_string());
            }
        }
    }
    Some(keys)
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeEntry {
    /// The key name or integer index. `None` for positional (unkeyed) entries.
    pub key: Option<String>,
    /// The value type of this field.
    pub value_type: PhpType,
    /// Whether this field is optional (`key?: type`).
    pub optional: bool,
}

/// A single parameter in a callable type specification.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallableParam {
    /// The type of this parameter.
    pub type_hint: PhpType,
    /// Whether the parameter is optional (has `=`).
    pub optional: bool,
    /// Whether the parameter is variadic (`...`).
    pub variadic: bool,
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/// Define keyword-type constructors that intern once and then hand out
/// clones.
///
/// Keyword types are by far the most-constructed forms, and interning them
/// on every call would pay a hash and a shard lock for an answer that never
/// changes. Holding the handle in a static also keeps these permanently
/// resident, which is where they belong: no analysis run is without `int`.
macro_rules! keyword_types {
    ($($(#[$doc:meta])* $name:ident => $text:literal,)*) => {
        impl PhpType {
            $(
                $(#[$doc])*
                pub fn $name() -> PhpType {
                    static CACHED: std::sync::LazyLock<PhpType> =
                        std::sync::LazyLock::new(|| PhpType::named(atom($text)));
                    CACHED.clone()
                }
            )*
        }
    };
}

keyword_types! {
    /// `int` type.
    int => "int",
    /// `string` type.
    string => "string",
    /// `float` type.
    float => "float",
    /// `bool` type.
    bool => "bool",
    /// `true` type.
    true_ => "true",
    /// `false` type.
    false_ => "false",
    /// `null` type.
    null => "null",
    /// `void` type.
    void => "void",
    /// `mixed` type.
    mixed => "mixed",
    /// `never` type.
    never => "never",
    /// `array` type (bare, unparameterised).
    array => "array",
    /// `object` type.
    object => "object",
    /// `callable` type.
    callable => "callable",
    /// `\Closure` type (fully-qualified).
    closure => "Closure",
    /// `iterable` type.
    iterable => "iterable",
    /// `self` type.
    self_ => "self",
    /// `static` type.
    static_ => "static",
    /// `$this` type.
    this => "$this",
    /// `parent` type.
    parent_ => "parent",
    /// `numeric` pseudo-type.
    numeric => "numeric",
    /// Internal `__empty` sentinel used during type narrowing to represent
    /// a fully-filtered-out union member.
    empty_sentinel => "__empty",
}

impl PhpType {
    /// Named type: a keyword, a class reference, or a template parameter.
    pub fn named(name: Atom) -> PhpType {
        TypeKind::Named(name).into()
    }

    /// `static` resolved against a class context: the runtime class, known
    /// to be at least `bound`.
    pub fn static_type(bound: Atom) -> PhpType {
        TypeKind::StaticType(bound).into()
    }

    /// `$this` resolved against a class context.
    pub fn this_type(bound: Atom) -> PhpType {
        TypeKind::ThisType(bound).into()
    }

    /// Nullable wrapper (`?T`).
    pub fn nullable(inner: PhpType) -> PhpType {
        TypeKind::Nullable(inner).into()
    }

    /// The `T[]` slice syntax.
    pub fn array_of(elem: PhpType) -> PhpType {
        TypeKind::Array(elem).into()
    }

    /// `key-of<T>`.
    pub fn key_of(inner: PhpType) -> PhpType {
        TypeKind::KeyOf(inner).into()
    }

    /// `value-of<T>`.
    pub fn value_of(inner: PhpType) -> PhpType {
        TypeKind::ValueOf(inner).into()
    }

    /// `class-string<T>`, or bare `class-string` when `inner` is `None`.
    pub fn class_string(inner: Option<PhpType>) -> PhpType {
        TypeKind::ClassString(inner).into()
    }

    /// `interface-string<T>`, or bare `interface-string` when `inner` is
    /// `None`.
    pub fn interface_string(inner: Option<PhpType>) -> PhpType {
        TypeKind::InterfaceString(inner).into()
    }

    /// Index access (`T[K]`).
    pub fn index_access(target: PhpType, index: PhpType) -> PhpType {
        TypeKind::IndexAccess(target, index).into()
    }

    /// A literal scalar (`42`, `'draft'`).
    pub fn literal(value: impl Into<Box<LiteralValue>>) -> PhpType {
        TypeKind::Literal(value.into()).into()
    }

    pub fn literal_int(raw: impl Into<Box<str>>) -> PhpType {
        PhpType::literal(LiteralValue::int(raw))
    }

    pub fn literal_float(raw: impl Into<Box<str>>) -> PhpType {
        PhpType::literal(LiteralValue::float(raw))
    }

    pub fn literal_string_raw(raw: impl Into<Box<str>>) -> PhpType {
        PhpType::literal(LiteralValue::string_raw(raw))
    }

    pub fn literal_string_value(value: impl AsRef<str>) -> PhpType {
        PhpType::literal(LiteralValue::string_value(value))
    }

    /// Convenience constructor for the "no type information" sentinel.
    ///
    /// Uses an empty `Raw` under the hood.  Prefer this over a bare
    /// `PhpType::raw("")` so the intent ("absence of type") is
    /// distinguishable from "unparseable input" at a glance.
    pub fn untyped() -> PhpType {
        static CACHED: std::sync::LazyLock<PhpType> = std::sync::LazyLock::new(|| PhpType::raw(""));
        CACHED.clone()
    }

    /// Fallback constructor for text we could not parse into a structured
    /// type.  Use [`untyped`](PhpType::untyped) for "no type given".
    pub fn raw(text: impl Into<Box<str>>) -> PhpType {
        TypeKind::Raw(text.into()).into()
    }

    /// Union of two or more members.
    ///
    /// Repeated alternatives are dropped (and a union left with a single
    /// alternative is unwrapped), because a union that names the same type
    /// twice is never anything but noise in a hover or a diagnostic. No
    /// other normalisation happens here — for `true|false` → `bool`,
    /// subtype absorption, and nested-union flattening see
    /// [`simplified`](PhpType::simplified).
    pub fn union(mut members: Vec<PhpType>) -> PhpType {
        if normalize::absorb_non_empty_refinements(&mut members) && members.len() == 1 {
            return members.into_iter().next().unwrap();
        }
        if normalize::has_duplicate_members(&members) {
            normalize::dedup_types(&mut members);
            if members.len() == 1 {
                return members.into_iter().next().unwrap();
            }
        }
        TypeKind::Union(members.into()).into()
    }

    /// Intersection of two or more members.
    pub fn intersection(members: Vec<PhpType>) -> PhpType {
        TypeKind::Intersection(members.into()).into()
    }

    /// Array shape (`array{…}`).
    pub fn array_shape(entries: Vec<ShapeEntry>) -> PhpType {
        TypeKind::ArrayShape(entries.into()).into()
    }

    /// List shape (`list{…}`): an array shape that also promises its keys
    /// are `0, 1, 2, …` in order.
    pub fn list_shape(entries: Vec<ShapeEntry>) -> PhpType {
        TypeKind::ListShape(PhpType::array_shape(entries)).into()
    }

    /// Tag `inner` as a list shape, if it is a shape at all.
    ///
    /// A transform that rebuilds an array shape uses this to carry the
    /// ordering promise across; anything else is returned untagged, which
    /// keeps the marker directly above the shape node the way
    /// [`kind`](PhpType::kind) assumes.
    pub(crate) fn as_list_shape(inner: PhpType) -> PhpType {
        if !matches!(inner.raw_kind(), TypeKind::ArrayShape(_)) {
            return inner;
        }
        TypeKind::ListShape(inner).into()
    }

    /// Object shape (`object{…}`).
    pub fn object_shape(entries: Vec<ShapeEntry>) -> PhpType {
        TypeKind::ObjectShape(entries.into()).into()
    }

    /// Parameterised type (`Collection<int, User>`).
    pub fn generic(name: impl AsRef<str>, args: Vec<PhpType>) -> PhpType {
        PhpType::generic_atom(atom(name.as_ref()), args)
    }

    /// Parameterised type from an already-interned base name.
    pub fn generic_atom(name: Atom, args: Vec<PhpType>) -> PhpType {
        TypeKind::Generic(Box::new(GenericType { name, args })).into()
    }

    /// Callable specification (`callable(int): string`).
    pub fn callable_spec(
        kind: impl AsRef<str>,
        params: Vec<CallableParam>,
        return_type: Option<PhpType>,
    ) -> PhpType {
        PhpType::callable_type(CallableType {
            kind: atom(kind.as_ref()),
            params,
            return_type,
        })
    }

    /// Callable specification from a prepared payload.
    pub fn callable_type(callable: CallableType) -> PhpType {
        TypeKind::Callable(Box::new(callable)).into()
    }

    /// Conditional type (`$x is T ? U : V`).
    pub fn conditional(
        param: impl AsRef<str>,
        negated: bool,
        condition: PhpType,
        then_type: PhpType,
        else_type: PhpType,
    ) -> PhpType {
        PhpType::conditional_type(ConditionalType {
            param: atom(param.as_ref()),
            negated,
            condition,
            then_type,
            else_type,
            else_when_undecided: false,
        })
    }

    /// Conditional type whose `then` branch needs proof.
    ///
    /// See [`ConditionalType::else_when_undecided`] for when to reach
    /// for this instead of [`PhpType::conditional`].
    pub fn conditional_defaulting_to_else(
        param: impl AsRef<str>,
        negated: bool,
        condition: PhpType,
        then_type: PhpType,
        else_type: PhpType,
    ) -> PhpType {
        PhpType::conditional_type(ConditionalType {
            param: atom(param.as_ref()),
            negated,
            condition,
            then_type,
            else_type,
            else_when_undecided: true,
        })
    }

    /// Conditional type from a prepared payload.
    pub fn conditional_type(conditional: ConditionalType) -> PhpType {
        TypeKind::Conditional(Box::new(conditional)).into()
    }

    /// Integer range (`int<0, max>`).
    pub fn int_range(min: impl AsRef<str>, max: impl AsRef<str>) -> PhpType {
        TypeKind::IntRange(atom(min.as_ref()), atom(max.as_ref())).into()
    }

    /// Returns `true` when this value represents the "no type" sentinel
    /// produced by [`PhpType::untyped()`].
    pub fn is_untyped(&self) -> bool {
        matches!(self.kind(), TypeKind::Raw(s) if s.is_empty())
    }

    /// `list<T>` generic type.
    pub fn list(elem: PhpType) -> PhpType {
        PhpType::generic("list", vec![elem])
    }

    /// `array<K, V>` generic type with explicit key and value types.
    pub fn generic_array(key: PhpType, val: PhpType) -> PhpType {
        PhpType::generic("array", vec![key, val])
    }

    /// `array<V>` generic type with only a value type (implicit integer key).
    pub fn generic_array_val(val: PhpType) -> PhpType {
        PhpType::generic("array", vec![val])
    }
}

impl PhpType {
    /// Whether this type represents "no type" (an empty `Raw` or `Named`
    /// variant whose display string would be empty).
    ///
    /// This avoids the `.to_string().is_empty()` round-trip when callers
    /// only need to know whether a `PhpType` carries meaningful content.
    pub fn is_empty(&self) -> bool {
        matches!(self.kind(), TypeKind::Raw(s) if s.is_empty())
            || matches!(self.kind(), TypeKind::Named(s) if s.is_empty())
    }

    /// Whether this type is the internal `__empty` sentinel used during
    /// type narrowing to represent a fully-filtered-out union member.
    pub fn is_empty_sentinel(&self) -> bool {
        matches!(self.kind(), TypeKind::Named(s) if s == "__empty")
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
        match self.kind() {
            TypeKind::Named(s) => is_primitive_scalar_name(s),
            TypeKind::Nullable(inner) => inner.is_primitive_scalar(),
            TypeKind::Generic(g) => is_primitive_scalar_name(&g.name),
            TypeKind::Array(_) => true,
            TypeKind::ArrayShape(_) => true,
            TypeKind::Callable(_) => true,
            TypeKind::IntRange(_, _) => true,
            TypeKind::Literal(_) => true,
            TypeKind::Raw(_) => false,
            _ => false,
        }
    }

    /// Whether this type is a bare, unparameterised primitive scalar name.
    ///
    /// Returns `true` only for simple `TypeKind::Named` values whose name
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
        matches!(self.kind(), TypeKind::Named(s) if is_primitive_scalar_name(s))
    }

    /// Whether this type admits `null` as a value.
    ///
    /// Returns `true` for `null` itself, a `?T` nullable wrapper, `mixed`,
    /// and any union that contains a null member. Returns `false` for
    /// non-nullable types.
    pub fn accepts_null(&self) -> bool {
        match self.kind() {
            TypeKind::Nullable(_) => true,
            TypeKind::Union(members) => members.iter().any(|m| m.accepts_null()),
            TypeKind::Named(s) => s.eq_ignore_ascii_case("null") || s.eq_ignore_ascii_case("mixed"),
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
        match self.kind() {
            TypeKind::Union(members) => {
                let mut members = members.to_vec();
                members.push(PhpType::null());
                PhpType::union(members)
            }
            _ => PhpType::nullable(self),
        }
    }

    /// Whether this type is a scalar/built-in type that does not refer
    /// to a user-defined class.
    ///
    /// Returns `true` when this type is exactly `null`.
    pub fn is_null(&self) -> bool {
        matches!(self.kind(), TypeKind::Named(s) if s.eq_ignore_ascii_case("null"))
    }

    /// Whether a [`TypeKind::Conditional`] appears anywhere in this type tree.
    ///
    /// Used as a cheap guard before running the (cloning) nested-conditional
    /// evaluator over a method's resolved return type: conditionals embedded
    /// inside a generic wrapper (e.g. `Collection<($x is array ? … : …), …>`)
    /// need to be collapsed against the call arguments, but the vast majority
    /// of return types contain no conditional and can be left untouched.
    pub fn contains_conditional(&self) -> bool {
        match self.kind() {
            TypeKind::Conditional(_) => true,
            TypeKind::Nullable(inner)
            | TypeKind::Array(inner)
            | TypeKind::ClassString(Some(inner))
            | TypeKind::InterfaceString(Some(inner))
            | TypeKind::KeyOf(inner)
            | TypeKind::ValueOf(inner) => inner.contains_conditional(),
            TypeKind::Union(members) | TypeKind::Intersection(members) => {
                members.iter().any(|m| m.contains_conditional())
            }
            TypeKind::Generic(g) => g.args.iter().any(|m| m.contains_conditional()),
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
                entries.iter().any(|e| e.value_type.contains_conditional())
            }
            TypeKind::Callable(c) => {
                c.params.iter().any(|p| p.type_hint.contains_conditional())
                    || c.return_type
                        .as_ref()
                        .is_some_and(|r| r.contains_conditional())
            }
            TypeKind::IndexAccess(base, index) => {
                base.contains_conditional() || index.contains_conditional()
            }
            _ => false,
        }
    }

    /// Whether a type operator that never got evaluated — `key-of<T>`,
    /// `value-of<T>`, `T[K]` — appears anywhere in this type tree.
    ///
    /// Companion guard to [`PhpType::unevaluated_operators_as_bounds`], in the
    /// same spirit as [`PhpType::contains_conditional`]: cheap enough to run on
    /// every type, so the (cloning) rewrite only runs on the few that need it.
    pub fn contains_unevaluated_operator(&self) -> bool {
        match self.kind() {
            TypeKind::KeyOf(_) | TypeKind::ValueOf(_) | TypeKind::IndexAccess(..) => true,
            TypeKind::Nullable(inner)
            | TypeKind::Array(inner)
            | TypeKind::ClassString(Some(inner))
            | TypeKind::InterfaceString(Some(inner)) => inner.contains_unevaluated_operator(),
            TypeKind::Union(members) | TypeKind::Intersection(members) => {
                members.iter().any(|m| m.contains_unevaluated_operator())
            }
            TypeKind::Generic(g) => g.args.iter().any(|m| m.contains_unevaluated_operator()),
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => entries
                .iter()
                .any(|e| e.value_type.contains_unevaluated_operator()),
            TypeKind::Callable(c) => {
                c.params
                    .iter()
                    .any(|p| p.type_hint.contains_unevaluated_operator())
                    || c.return_type
                        .as_ref()
                        .is_some_and(|r| r.contains_unevaluated_operator())
            }
            TypeKind::Conditional(c) => {
                c.condition.contains_unevaluated_operator()
                    || c.then_type.contains_unevaluated_operator()
                    || c.else_type.contains_unevaluated_operator()
            }
            _ => false,
        }
    }

    /// Collect the names an unevaluated type operator reads: the operand of
    /// `key-of<X>` / `value-of<X>` and the base of `X[K]`.
    ///
    /// An operator survives evaluation when its operand is a bare name, which
    /// is either a template nobody has substituted yet or a constant nobody
    /// has read yet — the docblock parser cannot tell the two apart. Callers
    /// that *can* read constants use this to learn which names a type needs
    /// resolved before they bind them (see
    /// `type_engine::call_resolution::constant_operand_shape`).
    ///
    /// Names are appended in traversal order, without deduplication; a class
    /// constant arrives in its written `Foo::BAR` spelling.
    pub fn unevaluated_operator_operands(&self, out: &mut Vec<String>) {
        let push_operand = |operand: &PhpType, out: &mut Vec<String>| match operand.kind() {
            TypeKind::Named(name) => out.push(name.to_string()),
            TypeKind::Raw(text) => out.push(text.to_string()),
            _ => operand.unevaluated_operator_operands(out),
        };
        match self.kind() {
            TypeKind::KeyOf(operand) | TypeKind::ValueOf(operand) => push_operand(operand, out),
            TypeKind::IndexAccess(base, index) => {
                push_operand(base, out);
                index.unevaluated_operator_operands(out);
            }
            TypeKind::Nullable(inner)
            | TypeKind::Array(inner)
            | TypeKind::ClassString(Some(inner))
            | TypeKind::InterfaceString(Some(inner)) => inner.unevaluated_operator_operands(out),
            TypeKind::Union(members) | TypeKind::Intersection(members) => {
                for member in members {
                    member.unevaluated_operator_operands(out);
                }
            }
            TypeKind::Generic(g) => {
                for arg in &g.args {
                    arg.unevaluated_operator_operands(out);
                }
            }
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
                for entry in entries {
                    entry.value_type.unevaluated_operator_operands(out);
                }
            }
            TypeKind::Callable(c) => {
                for param in &c.params {
                    param.type_hint.unevaluated_operator_operands(out);
                }
                if let Some(ret) = &c.return_type {
                    ret.unevaluated_operator_operands(out);
                }
            }
            TypeKind::Conditional(c) => {
                c.condition.unevaluated_operator_operands(out);
                c.then_type.unevaluated_operator_operands(out);
                c.else_type.unevaluated_operator_operands(out);
            }
            _ => {}
        }
    }

    /// Whether this type is `bool` or `boolean` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?bool` (nullable wrapper).
    pub fn is_bool(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => matches!(keyword_lowercase(s).as_str(), "bool" | "boolean"),
            TypeKind::Nullable(inner) => inner.is_bool(),
            _ => false,
        }
    }

    /// Whether this type is `true` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?true` (nullable wrapper).
    pub fn is_true(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("true"),
            TypeKind::Nullable(inner) => inner.is_true(),
            _ => false,
        }
    }

    /// Whether this type is `false` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?false` (nullable wrapper).
    pub fn is_false(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("false"),
            TypeKind::Nullable(inner) => inner.is_false(),
            _ => false,
        }
    }

    /// Whether this type is `int` or `integer` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?int` (nullable wrapper).
    pub fn is_int(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => matches!(keyword_lowercase(s).as_str(), "int" | "integer"),
            TypeKind::Nullable(inner) => inner.is_int(),
            _ => false,
        }
    }

    /// Whether this type is `string` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?string` (nullable wrapper).
    pub fn is_string_type(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("string"),
            TypeKind::Nullable(inner) => inner.is_string_type(),
            _ => false,
        }
    }

    /// Whether this type is `float` or `double` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?float` (nullable wrapper).
    pub fn is_float(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => {
                matches!(keyword_lowercase(s).as_str(), "float" | "double" | "real")
            }
            TypeKind::Nullable(inner) => inner.is_float(),
            _ => false,
        }
    }

    /// The literal payload, if this is a [`TypeKind::Literal`].
    pub fn as_literal(&self) -> Option<&LiteralValue> {
        match self.kind() {
            TypeKind::Literal(value) => Some(value),
            _ => None,
        }
    }

    /// Whether this type is a literal string value (e.g. `'hello'`, `"world"`).
    pub fn is_string_literal(&self) -> bool {
        matches!(self.as_literal(), Some(LiteralValue::String(_)))
    }

    /// Whether this type is a literal integer value (e.g. `42`, `-1`).
    pub fn is_int_literal(&self) -> bool {
        matches!(self.as_literal(), Some(LiteralValue::Int(_)))
    }

    /// Whether this type is `string` or any PHPDoc string refinement
    /// (case-insensitive), plus `ClassString(…)`, `InterfaceString(…)` and
    /// string literals.
    ///
    /// The list below is the whole string family from
    /// [`is_scalar_name`](crate::php_type::is_scalar_name_pub); leaving one
    /// out makes that spelling look like a different kind of type, and
    /// `is_compatible_refinement_typed` then discards it in favour of the
    /// bare `string` a parameter declares natively.
    pub fn is_string_subtype(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => matches!(
                s.to_ascii_lowercase().as_str(),
                "string"
                    | "non-empty-string"
                    | "numeric-string"
                    | "literal-string"
                    | "non-empty-literal-string"
                    | "truthy-string"
                    | "callable-string"
                    | "class-string"
                    | "interface-string"
                    | "trait-string"
                    | "enum-string"
                    | "lowercase-string"
                    | "non-empty-lowercase-string"
                    | "uppercase-string"
                    | "non-empty-uppercase-string"
                    | "non-falsy-string"
            ),
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => true,
            TypeKind::Literal(l) => matches!(**l, LiteralValue::String(_)),
            TypeKind::Nullable(inner) => inner.is_string_subtype(),
            TypeKind::Generic(g) => matches!(
                g.name.to_ascii_lowercase().as_str(),
                "class-string" | "interface-string" | "model-property"
            ),
            TypeKind::Union(members) => {
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
        match self.kind() {
            TypeKind::Named(s) => matches!(
                keyword_lowercase(s).as_str(),
                "int"
                    | "integer"
                    | "positive-int"
                    | "negative-int"
                    | "non-negative-int"
                    | "non-positive-int"
                    | "non-zero-int"
            ),
            TypeKind::IntRange(_, _) => true,
            TypeKind::Literal(l) => matches!(**l, LiteralValue::Int(_)),
            TypeKind::Nullable(inner) => inner.is_int_subtype(),
            TypeKind::Union(members) => {
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
        match self.kind() {
            TypeKind::Literal(l) => matches!(**l, LiteralValue::Float(_)),
            TypeKind::Union(members) => {
                !members.is_empty() && members.iter().all(|m| m.is_float_subtype())
            }
            _ => self.is_float(),
        }
    }

    /// Whether this type is `object` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?object` (nullable wrapper).
    pub fn is_object(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("object"),
            TypeKind::Nullable(inner) => inner.is_object(),
            _ => false,
        }
    }

    /// Whether this type is `array-key` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?array-key` (nullable wrapper).
    pub fn is_array_key(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("array-key"),
            TypeKind::Nullable(inner) => inner.is_array_key(),
            _ => false,
        }
    }

    /// Whether this type is `callable`, `Closure`, or a callable specification
    /// (case-insensitive).
    ///
    /// Also returns `true` when the type is `?callable` (nullable wrapper)
    /// or a `Callable { .. }` variant.
    pub fn is_callable(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => {
                let trimmed = s.strip_prefix('\\').unwrap_or(s);
                trimmed.eq_ignore_ascii_case("callable") || trimmed.eq_ignore_ascii_case("Closure")
            }
            TypeKind::Callable(_) => true,
            TypeKind::Nullable(inner) => inner.is_callable(),
            _ => false,
        }
    }

    /// Whether this type is `iterable` (case-insensitive).
    ///
    /// Also returns `true` when the type is `?iterable` (nullable wrapper).
    pub fn is_iterable(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("iterable"),
            TypeKind::Nullable(inner) => inner.is_iterable(),
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
        match self.kind() {
            TypeKind::Named(s) => {
                let trimmed = s.strip_prefix('\\').unwrap_or(s);
                trimmed.eq_ignore_ascii_case("Closure")
            }
            TypeKind::Callable(c) => c.kind.eq_ignore_ascii_case("Closure"),
            TypeKind::Nullable(inner) => inner.is_closure(),
            _ => false,
        }
    }

    /// Whether this type is `resource`.
    ///
    /// Only the lowercase spelling counts: PHP has no `resource` type-hint,
    /// so a project may legally declare a class named `Resource`, and that
    /// must not be folded into the pseudo-type (see
    /// `is_lowercase_only_pseudo_type`). Also returns `true` when the type
    /// is `?resource` (nullable wrapper).
    pub fn is_resource(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s == "resource",
            TypeKind::Nullable(inner) => inner.is_resource(),
            _ => false,
        }
    }

    /// Whether this type is a `Named` variant whose name equals `name`
    /// (case-sensitive comparison).
    ///
    /// Replaces the common `matches!(ty.kind(), TypeKind::Named(n) if n == name)`
    /// pattern used for template parameter identity checks.
    pub fn is_named(&self, name: &str) -> bool {
        matches!(self.kind(), TypeKind::Named(n) if n == name)
    }

    /// Whether this type is a `Named` variant whose name equals `name`
    /// (case-insensitive comparison).
    ///
    /// Replaces `matches!(ty.kind(), TypeKind::Named(n) if n.eq_ignore_ascii_case(name))`
    /// patterns.
    pub fn is_named_ci(&self, name: &str) -> bool {
        matches!(self.kind(), TypeKind::Named(n) if n.eq_ignore_ascii_case(name))
    }

    /// Returns `true` when this type is always coerced to `int` when
    /// used as an array key (int subtypes, float, and bool).
    ///
    /// Null is excluded because PHP converts it to the empty string key.
    pub fn is_int_coercible_key(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => {
                s == "number"
                    || matches!(
                        keyword_lowercase(s).as_str(),
                        "int"
                            | "integer"
                            | "float"
                            | "double"
                            | "real"
                            | "bool"
                            | "boolean"
                            | "true"
                            | "false"
                            | "positive-int"
                            | "negative-int"
                            | "non-negative-int"
                            | "non-positive-int"
                            | "non-zero-int"
                    )
            }
            TypeKind::Literal(value) => {
                matches!(**value, LiteralValue::Int(_) | LiteralValue::Float(_))
            }
            TypeKind::IntRange(_, _) => true,
            TypeKind::Union(members) => {
                !members.is_empty() && members.iter().all(PhpType::is_int_coercible_key)
            }
            _ => false,
        }
    }

    /// If this is a `Named` type that refers to a class (not a scalar,
    /// keyword, or pseudo-type), return its name.  Returns `None` for
    /// scalars (`int`, `string`, …), keywords (`mixed`, `void`, …),
    /// and non-`Named` variants.
    pub fn class_name(&self) -> Option<&str> {
        if let TypeKind::Named(name) = self.kind()
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
    /// returns `true` only for a bare `PhpType::named("self")` (and
    /// the other two variants).  Use this when you need to detect
    /// exactly the names that [`replace_self`] would rewrite, without
    /// unwrapping nullable/union layers.
    pub fn is_self_ref(&self) -> bool {
        matches!(self.kind(), TypeKind::Named(s) if is_self_ref_name(s))
    }

    /// Whether this type is one of the self-referencing keywords:
    /// `self`, `static`, `$this`, or `parent` (case-insensitive).
    ///
    /// Also returns `true` when the type is nullable (e.g. `?static`).
    /// Returns `true` when this type refers to the `parent` keyword
    /// (bare, nullable, or in a union with null).
    pub fn is_parent_ref(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("parent"),
            TypeKind::Generic(g) => g.name.eq_ignore_ascii_case("parent"),
            TypeKind::Nullable(inner) => inner.is_parent_ref(),
            TypeKind::Union(members) => {
                let non_null: Vec<_> = members.iter().filter(|m| !m.is_null()).collect();
                !non_null.is_empty() && non_null.iter().all(|m| m.is_parent_ref())
            }
            _ => false,
        }
    }

    pub fn is_self_like(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => self.is_self_ref() || s.eq_ignore_ascii_case("parent"),
            TypeKind::Generic(g) => {
                // e.g. `self<RuleError>`, `static<T>` — check the generic base name directly.
                // Cannot use `base_name()` here because it filters out self-like
                // names via `is_scalar_name`.
                is_self_ref_name(&g.name) || g.name.eq_ignore_ascii_case("parent")
            }
            TypeKind::Nullable(inner) => inner.is_self_like(),
            TypeKind::Union(members) => {
                // `static|null` — every non-null member is self-like.
                let non_null: Vec<_> = members.iter().filter(|m| !m.is_null()).collect();
                !non_null.is_empty() && non_null.iter().all(|m| m.is_self_like())
            }
            _ => false,
        }
    }

    /// Returns `true` for the shape with no entries — `array{}`, the type
    /// of an `[]` literal.
    ///
    /// It is the one array type whose value set is a single value, which
    /// makes it a member of every array type that does not demand an
    /// entry, and makes every offset read on it a guaranteed miss.
    pub fn is_empty_array_shape(&self) -> bool {
        matches!(self.kind(), TypeKind::ArrayShape(entries) if entries.is_empty())
    }

    /// Returns `true` when this array type has the empty array among its
    /// values, so an `array{}` alternative beside it is redundant.
    ///
    /// `array`, `array<K, V>`, `list<V>`, `T[]` and `iterable` all do. The
    /// `non-empty-*` family does not, and neither does a shape: `array{}`
    /// is the only shape the empty array satisfies, and every other one
    /// names an entry it lacks.
    pub fn accepts_empty_array(&self) -> bool {
        let name = match self.kind() {
            TypeKind::Array(_) => return true,
            TypeKind::Named(name) => name,
            TypeKind::Generic(generic) => &generic.name,
            _ => return false,
        };
        matches!(
            crate::php_type::keywords::keyword_lowercase(name).as_str(),
            "array" | "list" | "iterable"
        )
    }

    /// Returns `true` when this type is exactly the bare, unparameterised
    /// `array` keyword — i.e. `PhpType::named("array")`.
    ///
    /// Returns `false` for parameterised arrays (`array<int, string>`),
    /// array shapes (`array{key: string}`), slice syntax (`T[]`), `list`,
    /// `non-empty-array`, `iterable`, and any other array-like type.
    ///
    /// Use this when you need to distinguish a plain `array` return type
    /// (which carries no element-type information) from richer array types.
    pub fn is_bare_array(&self) -> bool {
        matches!(self.kind(), TypeKind::Named(s) if s.eq_ignore_ascii_case("array"))
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
        match self.kind() {
            TypeKind::Named(s) => is_array_like_name(s),
            TypeKind::Generic(g) => is_array_like_name(&g.name),
            TypeKind::Array(_) => true,
            TypeKind::ArrayShape(_) => true,
            TypeKind::Nullable(inner) => inner.is_array_like(),
            _ => false,
        }
    }

    /// Returns true when this type represents an object (class instance, object keyword, or object shape).
    pub fn is_object_like(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case("object") || !is_scalar_name(s),
            TypeKind::Generic(g) => !is_scalar_name(&g.name),
            TypeKind::ObjectShape(_) => true,
            TypeKind::StaticType(s) | TypeKind::ThisType(s) => !is_scalar_name(s),
            TypeKind::Nullable(inner) => inner.is_object_like(),
            _ => false,
        }
    }

    /// Matches built-in PHP types and common PHPDoc pseudo-types like
    /// `mixed`, `class-string`, etc.
    pub fn is_scalar(&self) -> bool {
        match self.kind() {
            TypeKind::Named(s) => is_scalar_name(s),
            TypeKind::Nullable(inner) => inner.is_scalar(),
            TypeKind::Generic(g) => is_scalar_name(&g.name),
            TypeKind::Array(_) => true,
            TypeKind::ArrayShape(_) => true,
            TypeKind::ObjectShape(_) => true,
            TypeKind::Callable(_) => true,
            TypeKind::ClassString(_) => true,
            TypeKind::InterfaceString(_) => true,
            TypeKind::KeyOf(_) => true,
            TypeKind::ValueOf(_) => true,
            TypeKind::IntRange(_, _) => true,
            TypeKind::Literal(_) => true,
            TypeKind::Raw(_) => false,
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
        match self.kind() {
            TypeKind::Generic(g) => {
                is_scalar_name(&g.name) && g.args.iter().all(|a| a.is_scalar_leaf())
            }
            TypeKind::Array(inner) => inner.is_scalar_leaf(),
            TypeKind::Nullable(inner) => inner.is_scalar_leaf(),
            // A shape is only a scalar leaf when every entry value is;
            // `array{price: Decimal}` yields the non-scalar `Decimal`
            // when indexed or iterated.
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
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
        match self.kind() {
            TypeKind::Named(s) if !is_scalar_name(s) => {
                Some(s.strip_prefix('\\').unwrap_or(s.as_str()))
            }
            TypeKind::StaticType(s) | TypeKind::ThisType(s) => {
                Some(s.strip_prefix('\\').unwrap_or(s.as_str()))
            }
            TypeKind::Generic(g) if !is_scalar_name(&g.name) => {
                Some(g.name.strip_prefix('\\').unwrap_or(g.name.as_str()))
            }
            TypeKind::Nullable(inner) => inner.base_name(),
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
        match self.raw_kind() {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => inner.to_native_hint(),
            TypeKind::Named(s) | TypeKind::StaticType(s) | TypeKind::ThisType(s) => {
                native_scalar_name(s).map(|n| n.to_string())
            }
            TypeKind::Generic(g) => {
                // Generic classes: strip the generic params.
                // `array<K,V>` → `array`, `Collection<T>` → `Collection`
                native_scalar_name(&g.name)
                    .map(|n| n.to_string())
                    .or_else(|| Some(g.name.to_string()))
            }
            TypeKind::Nullable(inner) => inner.to_native_hint().map(|n| format!("?{}", n)),
            TypeKind::Union(members) => {
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
            TypeKind::Intersection(members) => {
                let native: Vec<String> =
                    members.iter().filter_map(|m| m.to_native_hint()).collect();
                if native.len() != members.len() {
                    return None;
                }
                Some(native.join("&"))
            }
            TypeKind::Array(_) | TypeKind::ArrayShape(_) => Some("array".to_string()),
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => Some("string".to_string()),
            TypeKind::IntRange(_, _) => Some("int".to_string()),
            TypeKind::Literal(l) => Some(
                match **l {
                    LiteralValue::String(_) => "string",
                    LiteralValue::Int(_) => "int",
                    LiteralValue::Float(_) => "float",
                }
                .to_string(),
            ),
            TypeKind::ObjectShape(_) => Some("object".to_string()),
            TypeKind::Callable(c) => Some(c.kind.to_string()),
            // Conditionals, key-of, value-of, index-access, and raw
            // types have no native form.
            TypeKind::Conditional(_)
            | TypeKind::KeyOf(_)
            | TypeKind::ValueOf(_)
            | TypeKind::IndexAccess(_, _)
            | TypeKind::Raw(_) => None,
        }
    }

    /// Like [`to_native_hint`] but returns a structured [`PhpType`] instead of a string,
    /// avoiding a parse round-trip.
    pub fn to_native_hint_typed(&self) -> Option<PhpType> {
        match self.raw_kind() {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => {
                inner.to_native_hint_typed()
            }
            TypeKind::Named(s) | TypeKind::StaticType(s) | TypeKind::ThisType(s) => {
                native_scalar_name(s).map(|n| PhpType::named(atom(n)))
            }
            TypeKind::Generic(g) => {
                // Generic classes: strip the generic params.
                // `array<K,V>` → `array`, `Collection<T>` → `Collection`
                native_scalar_name(&g.name)
                    .map(|n| PhpType::named(atom(n)))
                    .or_else(|| Some(PhpType::named(g.name)))
            }
            TypeKind::Nullable(inner) => inner.to_native_hint_typed().map(PhpType::nullable),
            TypeKind::Union(members) => {
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
                    Some(PhpType::union(deduped))
                }
            }
            TypeKind::Intersection(members) => {
                let native: Vec<PhpType> = members
                    .iter()
                    .filter_map(|m| m.to_native_hint_typed())
                    .collect();
                if native.len() != members.len() {
                    return None;
                }
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
                    Some(PhpType::intersection(deduped))
                }
            }
            TypeKind::Array(_) | TypeKind::ArrayShape(_) => Some(PhpType::array()),
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => Some(PhpType::string()),
            TypeKind::IntRange(_, _) => Some(PhpType::int()),
            TypeKind::Literal(l) => Some(match **l {
                LiteralValue::String(_) => PhpType::string(),
                LiteralValue::Int(_) => PhpType::int(),
                LiteralValue::Float(_) => PhpType::float(),
            }),
            TypeKind::ObjectShape(_) => Some(PhpType::object()),
            TypeKind::Callable(c) => Some(PhpType::named(c.kind)),
            TypeKind::Conditional(_)
            | TypeKind::KeyOf(_)
            | TypeKind::ValueOf(_)
            | TypeKind::IndexAccess(_, _)
            | TypeKind::Raw(_) => None,
        }
    }

    /// Return the top-level union members if this is a union type,
    /// or a single-element slice containing `self` otherwise.
    pub fn union_members(&self) -> Vec<&PhpType> {
        match self.kind() {
            TypeKind::Union(members) => members.iter().collect(),
            _ => vec![self],
        }
    }

    /// Return the top-level intersection members if this is an intersection
    /// type, or a single-element slice containing `self` otherwise.
    pub fn intersection_members(&self) -> Vec<&PhpType> {
        match self.kind() {
            TypeKind::Intersection(members) => members.iter().collect(),
            _ => vec![self],
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
        match self.kind() {
            TypeKind::Array(inner) => {
                if skip_scalar && inner.is_scalar() {
                    None
                } else {
                    Some(inner)
                }
            }
            TypeKind::Generic(g) if !g.args.is_empty() => {
                let args = &g.args;
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
            TypeKind::Nullable(inner) => inner.extract_value_type(skip_scalar),
            TypeKind::Union(members) => members
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
        match self.kind() {
            TypeKind::Generic(g) if g.args.len() >= 2 => {
                let key = &g.args[0];
                if skip_scalar && key.is_scalar() {
                    None
                } else {
                    Some(key)
                }
            }
            TypeKind::Nullable(inner) => inner.extract_key_type(skip_scalar),
            TypeKind::Union(members) => {
                members.iter().find_map(|m| m.extract_key_type(skip_scalar))
            }
            _ => None,
        }
    }

    /// Return the key type produced by iterating this type, as an owned type.
    ///
    /// Unlike [`extract_key_type`](Self::extract_key_type), this joins key
    /// domains from every union member and makes the implicit integer keys of
    /// lists and `T[]` explicit.
    pub fn iterable_key_type(&self) -> Option<PhpType> {
        match self.kind() {
            TypeKind::Array(_) => Some(PhpType::int()),
            TypeKind::ArrayShape(entries) => {
                let keys: Vec<PhpType> = entries
                    .iter()
                    .map(|entry| match entry.key.as_deref() {
                        None => PhpType::int(),
                        // The parser currently stores a class-constant shape
                        // key only as its display spelling. Without resolving
                        // the constant, its runtime array key may be int or
                        // string (`Foo::class` is safely covered as a subset).
                        Some(key) if key.contains("::") => {
                            PhpType::union(vec![PhpType::int(), PhpType::string()])
                        }
                        Some(key) if is_decimal_int_array_key(key) => PhpType::int(),
                        Some(_) => PhpType::string(),
                    })
                    .collect();
                match keys.len() {
                    0 => None,
                    1 => keys.into_iter().next(),
                    _ => Some(PhpType::join_runtime_value_types(keys)),
                }
            }
            TypeKind::ObjectShape(entries) => {
                // Object property names, including numeric-looking ones, are
                // yielded as strings and never use PHP array-key coercion.
                (!entries.is_empty()).then(PhpType::string)
            }
            TypeKind::Generic(g) if g.args.len() >= 2 => Some(g.args[0].clone()),
            TypeKind::Generic(g)
                if g.args.len() == 1
                    && matches!(
                        g.name.to_ascii_lowercase().as_str(),
                        // A single-argument `array<T>` names its value type,
                        // leaving the same implicit integer keys as `T[]`.
                        "list" | "non-empty-list" | "array" | "non-empty-array"
                    ) =>
            {
                Some(PhpType::int())
            }
            TypeKind::Nullable(inner) => inner.iterable_key_type(),
            TypeKind::Union(members) => {
                let keys: Vec<PhpType> = members
                    .iter()
                    .filter_map(PhpType::iterable_key_type)
                    .collect();
                match keys.len() {
                    0 => None,
                    1 => keys.into_iter().next(),
                    _ => Some(PhpType::join_runtime_value_types(keys)),
                }
            }
            _ => None,
        }
    }

    /// Whether this type only names a value type, leaving the key domain
    /// open: `array<T>`, `non-empty-array<T>`, `T[]` and bare `array`.
    ///
    /// [`iterable_key_type`](Self::iterable_key_type) answers `int` for these
    /// because sequential keys are the common case and iterating them as
    /// `int` is the useful default. That default is a guess, though, and a
    /// caller that is about to *refine* the key domain (rather than read one
    /// element out of it) has to start from every key PHP allows, or the
    /// refinement contradicts the guess and is thrown away. `list<T>` is not
    /// open: it promises `int` keys.
    pub fn has_open_key_domain(&self) -> bool {
        match self.kind() {
            TypeKind::Array(_) => true,
            TypeKind::Generic(g) => {
                g.args.len() == 1
                    && matches!(
                        g.name.to_ascii_lowercase().as_str(),
                        "array" | "non-empty-array"
                    )
            }
            TypeKind::Named(name) => {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "array" | "non-empty-array"
                )
            }
            TypeKind::Nullable(inner) => inner.has_open_key_domain(),
            TypeKind::Union(members) => members.iter().any(PhpType::has_open_key_domain),
            _ => false,
        }
    }

    /// The generic array type a constant shape describes, dropping the
    /// per-key detail: `array{a: int, b: string}` → `array<string,
    /// int|string>`, `array{User, Order}` → `list<User|Order>`.
    ///
    /// Anything that is not an array shape is returned unchanged, so a
    /// caller that only needs a container it can read a key and value type
    /// off can pass any type through. An empty shape has no key or value
    /// type to name and becomes a bare `array`.
    pub fn generalized_array(&self) -> PhpType {
        let TypeKind::ArrayShape(entries) = self.kind() else {
            return self.clone();
        };
        if entries.is_empty() {
            return PhpType::array();
        }
        let Some(value) = self.iterable_element_type() else {
            return PhpType::array();
        };
        // Only an all-positional shape promises the `0, 1, 2, …` keys a
        // `list` does; the moment one entry is named the result has to
        // spell its key type out.
        if entries.iter().all(|e| e.key.is_none()) {
            return PhpType::list(value);
        }
        match self.iterable_key_type() {
            Some(key) => PhpType::generic_array(key, value),
            None => PhpType::generic_array_val(value),
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
        match self.kind() {
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
                let mut values: Vec<PhpType> = Vec::new();
                for entry in entries {
                    if !values.contains(&entry.value_type) {
                        values.push(entry.value_type.clone());
                    }
                }
                match values.len() {
                    0 => None,
                    1 => Some(values.into_iter().next().unwrap()),
                    _ => Some(PhpType::union(values)),
                }
            }
            TypeKind::Nullable(inner) => inner.iterable_element_type(),
            TypeKind::Union(members) => {
                let values: Vec<PhpType> = members
                    .iter()
                    .filter_map(PhpType::iterable_element_type)
                    .collect();
                match values.len() {
                    0 => None,
                    1 => values.into_iter().next(),
                    _ => Some(PhpType::join_runtime_value_types(values)),
                }
            }
            _ => self.extract_value_type(false).cloned(),
        }
    }

    /// Look up the value type for a specific key in an array shape.
    ///
    /// Given a parsed `array{name: string, user: User}` and key `"user"`,
    /// returns `Some(&PhpType::named("User"))`.
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
        self.shape_entry(key).map(|entry| &entry.value_type)
    }

    /// The shape entry `key` addresses, which [`shape_value_type`] reads the
    /// value type off.
    ///
    /// A caller that has to know whether the entry may be absent (an offset
    /// read of a missing key is `null`, an argument for it is not required)
    /// needs the entry itself rather than just its type.
    ///
    /// [`shape_value_type`]: PhpType::shape_value_type
    pub fn shape_entry(&self, key: &str) -> Option<&ShapeEntry> {
        match self.kind() {
            TypeKind::ArrayShape(entries) => {
                // First try an exact key match (handles named and explicit
                // numeric keys like `array{0: User, 1: Address}`).
                if let Some(entry) = entries.iter().find(|e| e.key.as_deref() == Some(key)) {
                    return Some(entry);
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
                                return Some(entry);
                            }
                            positional_idx += 1;
                        }
                    }
                }
                None
            }
            TypeKind::Nullable(inner) => inner.shape_entry(key),
            TypeKind::Union(members) => members.iter().find_map(|m| m.shape_entry(key)),
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
        match self.kind() {
            TypeKind::ArrayShape(entries) => {
                if let Some(entry) = entries.iter().find(|e| e.key.as_deref() == Some(key)) {
                    return if entry.optional {
                        Some(PhpType::nullable(entry.value_type.clone()))
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
                                    Some(PhpType::nullable(entry.value_type.clone()))
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
            TypeKind::Nullable(inner) => inner.extract_shape_key_type(key),
            TypeKind::Union(members) => {
                // Every alternative contributes its own entry: reading
                // offset 1 of `array{string, null}|array{string, Err}` is
                // `null|Err`, not whichever alternative comes first.
                let found: Vec<PhpType> = members
                    .iter()
                    .filter_map(|m| m.extract_shape_key_type(key))
                    .collect();
                (!found.is_empty()).then(|| PhpType::union(found))
            }
            _ => None,
        }
    }

    /// Return the shape entries if this is an `ArrayShape` or `ObjectShape`.
    ///
    /// Also handles nullable shapes by delegating to the inner type.
    /// Returns `None` for all other variants.
    pub fn shape_entries(&self) -> Option<&[ShapeEntry]> {
        match self.kind() {
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => Some(entries),
            TypeKind::Nullable(inner) => inner.shape_entries(),
            TypeKind::Union(members) => {
                // Find the first array/object shape member in the union.
                members.iter().find_map(|m| m.shape_entries())
            }
            _ => None,
        }
    }

    /// Return `true` if this type is an array shape (`array{…}`).
    ///
    /// Also returns `true` for nullable shapes written as either
    /// `?array{…}` or `array{…}|null`.
    pub fn is_array_shape(&self) -> bool {
        self.nullable_array_shape_parts().is_some()
    }

    /// Return `true` if this type is a list shape (`list{…}`), which an
    /// `array{…}` written with the same entries is not: only the former
    /// requires the keys to run `0, 1, 2, …` in order.
    pub fn is_list_shape(&self) -> bool {
        matches!(self.raw_kind(), TypeKind::ListShape(_))
    }

    /// Return `true` if this type is an object shape (`object{…}`).
    ///
    /// Also returns `true` for `?object{…}`.
    pub fn is_object_shape(&self) -> bool {
        match self.kind() {
            TypeKind::ObjectShape(_) => true,
            TypeKind::Nullable(inner) => inner.is_object_shape(),
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
    /// Handles nullable shapes written as `?array{…}` or
    /// `array{…}|null` on either side (the join is nullable when either
    /// side is). Returns `None` when either side is not an array shape or
    /// contains positional (unkeyed) entries — those are list-style shapes
    /// where a per-key join is not meaningful.
    pub fn join_shapes(&self, other: &PhpType) -> Option<PhpType> {
        let (left, left_nullable) = self.nullable_array_shape_parts()?;
        let (right, right_nullable) = other.nullable_array_shape_parts()?;
        let joined = PhpType::array_shape(Self::join_shape_entries(left, right)?);
        if left_nullable || right_nullable {
            Some(PhpType::nullable(joined))
        } else {
            Some(joined)
        }
    }

    /// Extract the one array shape this type describes, plus whether it
    /// also admits `null`.
    ///
    /// PHPDoc accepts both `?array{…}` and `array{…}|null`, and
    /// [`union`](Self::union) does not fold the second spelling into the
    /// first, so the two arrive here as different type kinds. Branch
    /// merging has to treat them as one type or every write to a nullable
    /// shape leaves a separate alternative behind.
    ///
    /// A union naming more than one shape, or anything besides a shape and
    /// `null`, has no single shape to answer with and returns `None`.
    fn nullable_array_shape_parts(&self) -> Option<(&[ShapeEntry], bool)> {
        match self.kind() {
            TypeKind::ArrayShape(entries) => Some((entries, false)),
            TypeKind::Nullable(inner) => {
                let (entries, _) = inner.nullable_array_shape_parts()?;
                Some((entries, true))
            }
            TypeKind::Union(members) => {
                let mut shape: Option<&[ShapeEntry]> = None;
                let mut nullable = false;
                for member in members {
                    if member.is_null() {
                        nullable = true;
                    } else if let TypeKind::ArrayShape(entries) = member.kind() {
                        if shape.replace(entries).is_some() {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                shape.map(|entries| (entries, nullable))
            }
            _ => None,
        }
    }

    /// Join two shape entry lists (see [`join_shapes`]).
    ///
    /// Entries are paired by the key they occupy at runtime, so a
    /// positional entry an append added beside named keys lines up with
    /// the index it sits at. Keys from `a` keep their order; keys only in
    /// `b` follow in `b`'s order. Returns `None` when either side's keys
    /// are not decidable, and when either side is a bare list of values:
    /// two literals written slot by slot describe unrelated arrays, and
    /// pairing their slots up would invent a row neither one holds.
    ///
    /// [`join_shapes`]: Self::join_shapes
    fn join_shape_entries(a: &[ShapeEntry], b: &[ShapeEntry]) -> Option<Vec<ShapeEntry>> {
        fn is_value_list(entries: &[ShapeEntry]) -> bool {
            entries.iter().any(|entry| entry.key.is_none())
                && !entries.iter().any(|entry| {
                    entry
                        .key
                        .as_deref()
                        .is_some_and(|k| k.parse::<i64>().is_err())
                })
        }
        if is_value_list(a) || is_value_list(b) {
            return None;
        }
        let keys_a = runtime_shape_keys(a)?;
        let keys_b = runtime_shape_keys(b)?;
        // An entry that may be absent no longer sits at the index a
        // positional spelling counts it out at, so it takes its key along.
        let entry =
            |source: &ShapeEntry, key: &String, value_type: PhpType, optional: bool| ShapeEntry {
                key: match &source.key {
                    Some(key) => Some(key.clone()),
                    None if optional => Some(key.clone()),
                    None => None,
                },
                value_type,
                optional,
            };
        let mut joined: Vec<ShapeEntry> = Vec::with_capacity(a.len().max(b.len()));
        for (ea, key) in a.iter().zip(&keys_a) {
            match keys_b.iter().position(|other| other == key) {
                Some(index) => joined.push(entry(
                    ea,
                    key,
                    Self::join_values(&ea.value_type, &b[index].value_type),
                    ea.optional || b[index].optional,
                )),
                None => joined.push(entry(ea, key, ea.value_type.clone(), true)),
            }
        }
        for (eb, key) in b.iter().zip(&keys_b) {
            if !keys_a.contains(key) {
                joined.push(entry(eb, key, eb.value_type.clone(), true));
            }
        }
        Some(joined)
    }

    /// Union two value types for a joined shape key.
    ///
    /// Equivalent members are kept once and nested shape members are
    /// joined rather than accumulated, so repeated merges cannot grow
    /// the value type without bound. A member whose produced values are
    /// already covered by another (`123` beside `int`) is dropped for the
    /// same reason: one branch writing a literal and another writing the
    /// base type must not leave both spellings behind.
    fn join_values(a: &PhpType, b: &PhpType) -> PhpType {
        use crate::php_type::normalize::is_runtime_value_subtype;

        if a.equivalent(b) {
            return a.clone();
        }
        let mut members: Vec<PhpType> = a.union_members().into_iter().cloned().collect();
        'incoming: for m in b.union_members() {
            for existing in members.iter_mut() {
                if existing.equivalent(m) {
                    continue 'incoming;
                }
                if is_runtime_value_subtype(m, existing) {
                    continue 'incoming;
                }
                if is_runtime_value_subtype(existing, m) {
                    *existing = m.clone();
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
            PhpType::union(members)
        }
    }

    /// Look up the value type for a specific property in an object shape.
    ///
    /// Given a parsed `object{name: string, user: User}` and key `"user"`,
    /// returns `Some(&PhpType::named("User"))`.
    ///
    /// Also handles nullable object shapes (`?object{…}`).
    ///
    /// Returns `None` if this is not an object shape or the property
    /// is not found.
    pub fn object_shape_property_type(&self, prop: &str) -> Option<&PhpType> {
        match self.kind() {
            TypeKind::ObjectShape(entries) => entries
                .iter()
                .find(|e| e.key.as_deref() == Some(prop))
                .map(|e| &e.value_type),
            TypeKind::Nullable(inner) => inner.object_shape_property_type(prop),
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
        match self.kind() {
            TypeKind::Callable(c) => Some(c.params.as_slice()),
            TypeKind::Nullable(inner) => inner.callable_param_types(),
            TypeKind::Union(members) => {
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
        match self.kind() {
            TypeKind::Callable(c) => c.return_type.as_ref(),
            TypeKind::Nullable(inner) => inner.callable_return_type(),
            TypeKind::Union(members) => {
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
        match self.kind() {
            TypeKind::Generic(g) if Self::short_name_of(&g.name) == "Generator" => {
                match g.args.get(2) {
                    Some(send) if skip_scalar && send.is_scalar() => None,
                    Some(send) => Some(send),
                    None => None,
                }
            }
            TypeKind::Nullable(inner) => inner.generator_send_type(skip_scalar),
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
        match self.kind() {
            TypeKind::Nullable(inner) => Some(inner.clone()),
            TypeKind::Union(members) => {
                let non_null: Vec<&PhpType> = members.iter().filter(|m| !m.is_null()).collect();
                match non_null.len() {
                    0 => None,
                    1 => Some(non_null[0].clone()),
                    _ => Some(PhpType::union(non_null.into_iter().cloned().collect())),
                }
            }
            // Not a union or nullable — no null to strip.
            _ => None,
        }
    }

    /// Whether every value of this type is truthy, or none of them is.
    ///
    /// `None` for the types that span both (`string` holds `''`, `int`
    /// holds `0`, `array` holds `[]`), which leaves a caller reasoning
    /// about a truthiness test undecided rather than guessing.
    ///
    /// This is a single atomic member's truthiness: a union or nullable
    /// spans its members and is therefore undecided.
    pub fn truthiness(&self) -> Option<bool> {
        if self.is_null() || self.is_false() {
            return Some(false);
        }
        if self.is_true() {
            return Some(true);
        }
        match self.kind() {
            // PHP's own cast-to-bool rules, read off the literal's value
            // rather than its source text: `''` and `'0'` are the only falsy
            // strings, so `'0.0'` and `' '` are truthy.  A number the parsers
            // cannot read is one no `0` spelling produces, so it counts as
            // truthy rather than leaving the whole branch undecided.
            TypeKind::Literal(value) => Some(match value.as_ref() {
                LiteralValue::String(_) => value
                    .string_content()
                    .is_none_or(|text| !text.is_empty() && text != "0"),
                LiteralValue::Int(_) => value.parse_i64().is_none_or(|value| value != 0),
                LiteralValue::Float(_) => value.parse_f64().is_none_or(|value| value != 0.0),
            }),
            TypeKind::Named(name) => match name.to_ascii_lowercase().as_str() {
                "object" | "non-empty-string" | "non-empty-array" | "non-empty-list"
                | "positive-int" | "negative-int" | "callable" | "closure" => Some(true),
                other if is_keyword_type(other) => None,
                // A class instance is always truthy in PHP.
                _ => Some(true),
            },
            // An object shape or an intersection is an object, and an object
            // is always truthy.  A shape with at least one required field is
            // a non-empty array; an empty one (`array{}`) is falsy.
            TypeKind::Intersection(_) | TypeKind::ObjectShape(_) => Some(true),
            TypeKind::ArrayShape(_) | TypeKind::ListShape(_) => self
                .shape_entries()
                .map(|entries| entries.iter().any(|entry| !entry.optional)),
            _ => None,
        }
    }

    /// Return the part of a type that can be truthy.
    ///
    /// For `string|false` returns `Some(Named("string"))`, for `?User`
    /// `Some(Named("User"))`, and for `null`, `false`, or `?false` `None`,
    /// since nothing those describe survives a truthiness check.
    ///
    /// Members that are *always* falsy are dropped and `bool` keeps only
    /// its `true` half, which is the subset a truthy `if ($x)` branch
    /// narrows a variable to. Refinements a truthy test could also justify
    /// but that PHP has no plain spelling for are left alone: `int` stays
    /// `int` rather than becoming a range excluding `0`.
    pub fn truthy_type(&self) -> Option<PhpType> {
        match self.kind() {
            TypeKind::Nullable(inner) => inner.truthy_type(),
            // Each member is stripped by recursing rather than by checking
            // only whether the member as a whole is certainly falsy: a
            // `?int` member sitting inside a wider union (`?int|?bool`,
            // built by merging several properties into one array's value
            // type) is not certainly falsy on its own, so a shallow check
            // would leave its `null` in place while a bare `?int` handed to
            // this function directly loses it via the `Nullable` arm above.
            // Recursing makes every member go through the exact same rule
            // regardless of what else shares the union with it.
            TypeKind::Union(members) => {
                let truthy: Vec<PhpType> =
                    members.iter().filter_map(PhpType::truthy_type).collect();
                match truthy.len() {
                    0 => None,
                    1 => truthy.into_iter().next(),
                    _ => Some(PhpType::union(truthy)),
                }
            }
            _ if self.truthiness() == Some(false) => None,
            // A `bool` that survives a truthy test can only have been
            // `true`, and keeping it as `bool` makes the check
            // unfalsifiable: a variable seeded `false` and reassigned in
            // one branch would go on carrying its falsy half for the rest
            // of the scope.
            _ if self.is_bool() => Some(PhpType::true_()),
            // `[]` is the only falsy array, so an array that survives a
            // truthy test has at least one entry.  Carrying that in the
            // type is what lets a later `foreach` know its body runs.
            _ if self.is_array_like() => Some(self.non_empty_array_form()),
            _ => Some(self.clone()),
        }
    }

    /// Return the part of a type that can be falsy.
    ///
    /// The mirror of [`Self::truthy_type`], for the branch an `if ($x)`
    /// skips rather than the one it enters. Members that are *always*
    /// truthy are dropped and `bool` keeps only its `false` half; `None`
    /// comes back when nothing the type describes could have been falsy.
    ///
    /// As on the truthy side, refinements the test justifies but PHP has
    /// no plain spelling for are left alone: `string` stays `string`
    /// rather than becoming the pair of empty spellings that are falsy,
    /// and `int` stays `int` rather than becoming `0`.
    pub fn falsy_type(&self) -> Option<PhpType> {
        let mut falsy = Vec::new();
        self.push_falsy_members(&mut falsy);
        match falsy.len() {
            0 => None,
            1 => falsy.pop(),
            _ => Some(PhpType::union(falsy)),
        }
    }

    /// Append this type's falsy members to `out`.
    ///
    /// The unions and nullables a type is built from are flattened on the
    /// way, so [`Self::union`] is handed one list of atoms and can
    /// deduplicate the `null` that every nullable member contributes:
    /// `?int|?string` is falsy as `int|null|string`, not as a union with
    /// the same `null` in it twice.
    fn push_falsy_members(&self, out: &mut Vec<PhpType>) {
        match self.kind() {
            // `null` is falsy, so it survives whatever the inner type does.
            TypeKind::Nullable(inner) => {
                inner.push_falsy_members(out);
                out.push(PhpType::null());
            }
            TypeKind::Union(members) => {
                for member in members.iter() {
                    member.push_falsy_members(out);
                }
            }
            _ if self.truthiness() == Some(true) => {}
            _ if self.is_bool() => out.push(PhpType::false_()),
            _ => out.push(self.clone()),
        }
    }

    /// The non-empty counterpart of an array type: `array<K, V>` becomes
    /// `non-empty-array<K, V>` and `list<T>` becomes `non-empty-list<T>`.
    /// The `T[]` slice spelling is sugar for `array<T>` and refines the
    /// same way.
    ///
    /// Everything else comes back unchanged, including a shape (its own
    /// entries already say whether it can be empty) and a type that is
    /// non-empty by name already.
    pub fn non_empty_array_form(&self) -> PhpType {
        match self.kind() {
            TypeKind::Named(name) if name == "array" => PhpType::named(atom("non-empty-array")),
            TypeKind::Named(name) if name == "list" => PhpType::named(atom("non-empty-list")),
            TypeKind::Generic(generic) if generic.name == "array" => {
                PhpType::generic_atom(atom("non-empty-array"), generic.args.clone())
            }
            TypeKind::Generic(generic) if generic.name == "list" => {
                PhpType::generic_atom(atom("non-empty-list"), generic.args.clone())
            }
            TypeKind::Array(element) => {
                PhpType::generic_atom(atom("non-empty-array"), vec![element.clone()])
            }
            _ => self.clone(),
        }
    }

    /// Whether this type promises at least one entry.
    ///
    /// An optional entry (`array{a?: int}`) can be the only one there is,
    /// so a shape only counts once one of its entries is required, and a
    /// union only counts once every member does.
    pub fn is_provably_non_empty(&self) -> bool {
        match self.kind() {
            TypeKind::Named(name) => is_non_empty_array_name(name.as_str()),
            TypeKind::Generic(generic) => is_non_empty_array_name(generic.name.as_str()),
            TypeKind::ArrayShape(_) | TypeKind::ListShape(_) => self
                .shape_entries()
                .is_some_and(|entries| entries.iter().any(|entry| !entry.optional)),
            TypeKind::Union(members) => members.iter().all(PhpType::is_provably_non_empty),
            _ => false,
        }
    }

    /// The type left behind after `unset($var[$key])` (or an element unset
    /// off a nested array-access chain) removes one entry.
    ///
    /// `key` is the literal string key being removed, when known — `None`
    /// for a dynamic key (`unset($arr[$i])`). A non-array-like type (an
    /// `ArrayAccess` object routes through `offsetUnset` instead of real
    /// array mutation) comes back unchanged.
    ///
    /// A shape drops the matching entry outright when the key is known,
    /// since that key provably no longer exists. A dynamic key could have
    /// removed any one of the shape's entries, so every entry becomes
    /// optional instead — none of them is provably still there. Either way
    /// a shape that loses its only required entries is no longer provably
    /// non-empty, which is what lets a following `foreach` know its body
    /// might not run. Losing an entry also breaks a list's sequential-key
    /// promise, so a list shape is demoted to a plain array shape.
    ///
    /// A `non-empty-array`/`non-empty-list` (tracked by name, not by
    /// entries) loses that promise unconditionally: removing any one
    /// element could have emptied it out entirely.
    pub fn after_element_unset(&self, key: Option<&str>) -> PhpType {
        match self.kind() {
            TypeKind::Nullable(inner) => PhpType::nullable(inner.after_element_unset(key)),
            TypeKind::Union(members) => {
                PhpType::union(members.iter().map(|m| m.after_element_unset(key)).collect())
            }
            TypeKind::Named(name) if name == "non-empty-array" => PhpType::named(atom("array")),
            TypeKind::Named(name) if name == "non-empty-list" => PhpType::named(atom("list")),
            TypeKind::Generic(generic) if generic.name == "non-empty-array" => {
                PhpType::generic_atom(atom("array"), generic.args.clone())
            }
            TypeKind::Generic(generic) if generic.name == "non-empty-list" => {
                PhpType::generic_atom(atom("list"), generic.args.clone())
            }
            TypeKind::ArrayShape(entries) => {
                let updated: Vec<ShapeEntry> = match key {
                    Some(key) => entries
                        .iter()
                        .filter(|entry| entry.key.as_deref() != Some(key))
                        .cloned()
                        .collect(),
                    None => entries
                        .iter()
                        .cloned()
                        .map(|mut entry| {
                            entry.optional = true;
                            entry
                        })
                        .collect(),
                };
                PhpType::array_shape(updated)
            }
            _ => self.clone(),
        }
    }

    /// Unwrap one layer of `Nullable`, returning the inner type.
    ///
    /// For `Nullable(inner)` returns `inner`, for everything else returns `self`.
    /// This is a cheap, borrowing alternative to [`non_null_type`] which
    /// returns an owned `PhpType` and also handles union-with-null.
    pub fn unwrap_nullable(&self) -> &PhpType {
        match self.kind() {
            TypeKind::Nullable(inner) => inner,
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
        match self.kind() {
            TypeKind::Union(members) => members.to_vec(),
            TypeKind::Nullable(inner) => {
                vec![inner.clone(), PhpType::null()]
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
        match self.kind() {
            TypeKind::Union(members) => members
                .iter()
                .filter(|m| !m.is_null())
                .all(|m| m.is_scalar()),
            TypeKind::Nullable(inner) => inner.is_scalar(),
            _ => self.is_scalar(),
        }
    }

    /// If this is a `class-string<T>`, returns `Some(&T)`. Otherwise, returns `None`.
    pub fn unwrap_class_string_inner(&self) -> Option<&PhpType> {
        match self.kind() {
            TypeKind::ClassString(Some(inner)) => Some(inner),
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
        match self.kind() {
            TypeKind::Union(members) => members
                .iter()
                .filter(|m| !m.is_null())
                .all(|m| m.is_primitive_scalar()),
            TypeKind::Nullable(inner) => inner.is_primitive_scalar(),
            _ => self.is_primitive_scalar(),
        }
    }

    /// Whether this type carries structural information beyond a bare
    /// class name or scalar keyword.
    ///
    /// Returns `true` for generics, shapes, arrays, callables,
    /// class-string, key-of, value-of, conditionals, index access,
    /// int ranges, and literals.  Returns `false` for plain `Named`,
    /// `Raw`, and `Nullable(Named(_))`.
    pub fn has_type_structure(&self) -> bool {
        match self.kind() {
            TypeKind::Named(_) | TypeKind::Raw(_) => false,
            TypeKind::Nullable(inner) => inner.has_type_structure(),
            TypeKind::Union(members) => members.iter().any(|m| m.has_type_structure()),
            TypeKind::Intersection(members) => members.iter().any(|m| m.has_type_structure()),
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
    /// Operates on the structured type directly, avoiding a
    /// parse→check round-trip when the caller already has a `PhpType`.
    pub fn is_informative(&self) -> bool {
        match self.raw_kind() {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => inner.is_informative(),
            TypeKind::Generic(..) => true,
            TypeKind::ArrayShape(..) | TypeKind::ObjectShape(..) => true,
            TypeKind::Array(..) => true,
            TypeKind::Union(members) => members.iter().any(|m| m.is_informative()),
            TypeKind::Nullable(inner) => inner.is_informative(),
            TypeKind::Intersection(members) => members.iter().any(|m| m.is_informative()),
            TypeKind::Named(_) => {
                !(self.is_bare_array()
                    || self.is_mixed()
                    || self.is_object()
                    || self.is_void()
                    || self.is_null()
                    || self.is_self_like())
            }
            TypeKind::Callable(..) => true,
            TypeKind::ClassString(..) | TypeKind::InterfaceString(..) => true,
            TypeKind::KeyOf(..) | TypeKind::ValueOf(..) => true,
            TypeKind::IndexAccess(..) => true,
            TypeKind::Conditional(..) => true,
            TypeKind::IntRange(..) => true,
            TypeKind::Literal(..) => true,
            TypeKind::StaticType(_) | TypeKind::ThisType(_) => true,
            TypeKind::Raw(s) => s.contains('<') || s.contains('{') || s.ends_with("[]"),
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
        match self.kind() {
            TypeKind::Generic(..) => true,
            TypeKind::Array(..) => true,
            TypeKind::Nullable(inner) => inner.has_type_parameters(),
            TypeKind::Union(members) | TypeKind::Intersection(members) => {
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
        match self.raw_kind() {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => {
                inner.references_any_template_param(template_params)
            }
            TypeKind::Named(name) => template_params.iter().any(|p| p == name),
            TypeKind::Nullable(inner) => inner.references_any_template_param(template_params),
            TypeKind::Union(members) | TypeKind::Intersection(members) => members
                .iter()
                .any(|m| m.references_any_template_param(template_params)),
            TypeKind::Generic(g) => {
                template_params
                    .iter()
                    .any(|p| p.as_str() == g.name.as_str())
                    || g.args
                        .iter()
                        .any(|a| a.references_any_template_param(template_params))
            }
            TypeKind::Array(inner) => inner.references_any_template_param(template_params),
            TypeKind::ClassString(Some(inner)) | TypeKind::InterfaceString(Some(inner)) => {
                inner.references_any_template_param(template_params)
            }
            TypeKind::KeyOf(inner) | TypeKind::ValueOf(inner) => {
                inner.references_any_template_param(template_params)
            }
            TypeKind::Conditional(c) => {
                c.condition.references_any_template_param(template_params)
                    || c.then_type.references_any_template_param(template_params)
                    || c.else_type.references_any_template_param(template_params)
            }
            TypeKind::Callable(c) => {
                c.params
                    .iter()
                    .any(|p| p.type_hint.references_any_template_param(template_params))
                    || c.return_type
                        .as_ref()
                        .is_some_and(|r| r.references_any_template_param(template_params))
            }
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => entries
                .iter()
                .any(|e| e.value_type.references_any_template_param(template_params)),
            TypeKind::IndexAccess(base, index) => {
                base.references_any_template_param(template_params)
                    || index.references_any_template_param(template_params)
            }
            TypeKind::ClassString(None)
            | TypeKind::InterfaceString(None)
            | TypeKind::IntRange(..)
            | TypeKind::Literal(..)
            | TypeKind::StaticType(_)
            | TypeKind::ThisType(_)
            | TypeKind::Raw(..) => false,
        }
    }

    // -----------------------------------------------------------------------
    // Helpers for subtype / simplification
    // -----------------------------------------------------------------------

    /// Whether this type is `never` (bottom type).
    pub fn is_never(&self) -> bool {
        matches!(self.kind(), TypeKind::Named(s)
            if matches!(s.to_ascii_lowercase().as_str(),
                "never" | "no-return" | "noreturn" | "never-return" | "never-returns"
            )
        )
    }

    /// Whether this type is `mixed` (top type).
    pub fn is_mixed(&self) -> bool {
        matches!(self.kind(), TypeKind::Named(s) if s.eq_ignore_ascii_case("mixed"))
    }

    /// Whether `mixed` reaches the top level of this type.
    ///
    /// `mixed` is the top type, so a union or nullable that holds it holds
    /// every value: `mixed|Foo` and `?mixed` are both just `mixed`.
    /// [`simplified`](Self::simplified) folds them, but a caller that only
    /// needs the answer and not the folded type gets it here without
    /// rebuilding every member on the way.
    pub fn contains_mixed(&self) -> bool {
        if self.is_mixed() {
            return true;
        }
        match self.kind() {
            TypeKind::Union(members) => members.iter().any(PhpType::contains_mixed),
            TypeKind::Nullable(inner) => inner.contains_mixed(),
            _ => false,
        }
    }

    /// Whether this type is `void`.
    pub fn is_void(&self) -> bool {
        matches!(self.kind(), TypeKind::Named(s) if s.eq_ignore_ascii_case("void"))
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
        match self.kind() {
            TypeKind::Named(name) => is_keyword_type(name),
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../php_type_tests.rs"]
mod tests;
