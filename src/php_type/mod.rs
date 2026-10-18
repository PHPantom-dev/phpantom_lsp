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

mod components;
mod constructors;
mod display;
mod intern;
mod keywords;
mod literal;
mod normalize;
mod parse;
mod predicates;
mod runtime_join;
mod shape_keys;
mod shapes;
mod subtype;
mod transform;
mod truthiness;

#[cfg(any(test, feature = "mem-audit"))]
pub(crate) use intern::interned_len;
pub(crate) use intern::{FxHasher, PassThroughHasher};
pub(crate) use keywords::*;
pub use literal::LiteralValue;
pub(crate) use normalize::*;
pub(crate) use parse::*;
pub(crate) use shape_keys::*;
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
    ///
    /// A [`TemplateParam`](TypeKind::TemplateParam) is seen through to its
    /// bound the same way, which may take one more hop, and an
    /// [`UnsealedShape`](TypeKind::UnsealedShape) to the generic array it
    /// widens to.
    #[inline]
    pub fn kind(&self) -> &TypeKind {
        match &*self.0 {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => &inner.0,
            TypeKind::TemplateParam(_, bound) => bound.kind(),
            TypeKind::UnsealedShape(unsealed) => &unsealed.widened.0,
            TypeKind::ClassNameLiteral(class_string) => &class_string.0,
            kind => kind,
        }
    }

    /// The class this type is exactly the name of, when it is a
    /// [`ClassNameLiteral`](TypeKind::ClassNameLiteral).
    #[inline]
    pub fn as_class_name_literal(&self) -> Option<&str> {
        let TypeKind::ClassNameLiteral(class_string) = &*self.0 else {
            return None;
        };
        match class_string.kind() {
            TypeKind::ClassString(Some(class)) => match class.kind() {
                TypeKind::Named(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The unsealed shape this type is, when it is a
    /// [`UnsealedShape`](TypeKind::UnsealedShape).
    #[inline]
    pub fn as_unsealed_shape(&self) -> Option<&UnsealedShape> {
        match &*self.0 {
            TypeKind::UnsealedShape(unsealed) => Some(unsealed),
            _ => None,
        }
    }

    /// The template parameter this type is, and the bound it stands for,
    /// when it is a [`TemplateParam`](TypeKind::TemplateParam).
    #[inline]
    pub fn as_template_param(&self) -> Option<(Atom, &PhpType)> {
        match &*self.0 {
            TypeKind::TemplateParam(name, bound) => Some((*name, bound)),
            _ => None,
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

    /// A function or method `@template T of Bound` parameter, seen from
    /// inside the declaration that introduces it: the value is still `T`,
    /// but all that is known about it is that it is a `Bound`.
    ///
    /// Invisible to [`PhpType::kind`] the same way
    /// [`Benevolent`](TypeKind::Benevolent) is: every `match ty.kind()`
    /// sees the bound, so class lookup, member access, subtyping and
    /// narrowing all treat the value as its bound, and the bound travels
    /// with the value into anything derived from it (`array<T>`'s element,
    /// a foreach variable) however late that is looked at.  Only the
    /// display (which spells the template's name) and template
    /// substitution (which replaces it by name, like a `Named`) look at
    /// the marker itself.  A transform that rebuilds the type through
    /// `kind()` drops the name and keeps the bound, which is what it was
    /// before the marker existed.
    TemplateParam(Atom, PhpType),

    /// An unsealed shape: `array{a: A, ...<K, V>}` or `list{A, ...<V>}`.
    /// The entries it lists are known to be there, and any number of
    /// others, keyed `K` and holding `V`, may sit beside them.
    ///
    /// Invisible to [`PhpType::kind`], which sees the generic array the
    /// shape widens to (`non-empty-array<'a'|K, A|V>`, spelled with the
    /// key types rather than the literals). Code that knows nothing about
    /// the tail therefore treats it as the `array<K, V>` it also is, which
    /// is what it was before the type existed and is never wrong. Only the
    /// code that asks, through [`PhpType::as_unsealed_shape`], gets to read
    /// the listed entries one by one: offset reads, key completion and the
    /// display.
    UnsealedShape(Box<UnsealedShape>),

    /// `Foo::class`: the name of the class `Foo` itself, which the inner
    /// `class-string<Foo>` also admits the name of any subclass beside.
    ///
    /// Invisible to [`PhpType::kind`] and the display, which both see the
    /// `class-string<Foo>`, so the value goes wherever a class-string does.
    /// Only a write through it asks, via
    /// [`PhpType::as_class_name_literal`], since knowing the one name it
    /// holds is what lets the write name the entry it lands on.
    ClassNameLiteral(PhpType),
}

/// Payload of [`TypeKind::UnsealedShape`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnsealedShape {
    /// The entries known to be there, as an `array{…}`, or a `list{…}`
    /// when the whole array is a list.
    pub shape: PhpType,
    /// The key type of the entries beyond the listed ones.
    pub key: PhpType,
    /// The value type of the entries beyond the listed ones.
    pub value: PhpType,
    /// What [`PhpType::kind`] answers for the whole shape.
    widened: PhpType,
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

/// An array shape read the way two shapes are compared: the entries it
/// lists, and the tail an unsealed one adds beside them.
///
/// [`PhpType::kind`] reads an unsealed shape as the generic array it widens
/// to, which has lost the entries it lists, so a comparison of shapes goes
/// through [`PhpType::shape_parts`] instead.
#[derive(Clone, Copy)]
pub(crate) struct ShapeParts<'a> {
    /// The entries the shape lists.
    pub entries: &'a [ShapeEntry],
    /// The key and value types of the entries beyond the listed ones when
    /// the shape is unsealed, `None` when it is sealed.
    pub tail: Option<(&'a PhpType, &'a PhpType)>,
    /// Whether the shape promises its keys run `0, 1, 2, …` in order, which
    /// is what `list{…}` adds to `array{…}`.
    pub is_list: bool,
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

#[cfg(test)]
#[path = "../php_type_tests.rs"]
mod tests;
