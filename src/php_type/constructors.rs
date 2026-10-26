//! Constructors for every [`PhpType`] form, each going through the
//! interner.

use super::*;

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
    /// `non-empty-string` type.
    non_empty_string => "non-empty-string",
    /// `non-falsy-string` type.
    non_falsy_string => "non-falsy-string",
    /// `literal-string` type.
    literal_string => "literal-string",
    /// `non-empty-literal-string` type.
    non_empty_literal_string => "non-empty-literal-string",
    /// `numeric-string` type.
    numeric_string => "numeric-string",
    /// `lowercase-string` type.
    lowercase_string => "lowercase-string",
    /// `non-empty-lowercase-string` type.
    non_empty_lowercase_string => "non-empty-lowercase-string",
    /// `uppercase-string` type.
    uppercase_string => "uppercase-string",
    /// `non-empty-uppercase-string` type.
    non_empty_uppercase_string => "non-empty-uppercase-string",
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

    /// The template parameter `name`, known to be a `bound`.
    ///
    /// A bound that is itself a bounded template (`@template U of T`)
    /// contributes its own bound, so the marker never wraps another one and
    /// [`kind`](PhpType::kind) stays a bounded number of hops.
    pub fn template_param(name: Atom, bound: PhpType) -> PhpType {
        let bound = match bound.as_template_param() {
            Some((_, inner)) => inner.clone(),
            None => bound,
        };
        TypeKind::TemplateParam(name, bound).into()
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
    ///
    /// The class of an enum case is its enum, so a `T` bound to a case
    /// names the enum here.
    pub fn class_string(inner: Option<PhpType>) -> PhpType {
        TypeKind::ClassString(inner.map(|inner| inner.enum_cases_as_enums())).into()
    }

    /// `Foo::class`: exactly the name of `class`.
    pub fn class_name_literal(class: Atom) -> PhpType {
        TypeKind::ClassNameLiteral(PhpType::class_string(Some(PhpType::named(class)))).into()
    }

    /// `Suit::Hearts`: the case `case` of the enum `enum_name`.
    pub fn enum_case(enum_name: Atom, case: Atom) -> PhpType {
        TypeKind::EnumCase(PhpType::named(enum_name), case).into()
    }

    /// `interface-string<T>`, or bare `interface-string` when `inner` is
    /// `None`.
    pub fn interface_string(inner: Option<PhpType>) -> PhpType {
        TypeKind::InterfaceString(inner.map(|inner| inner.enum_cases_as_enums())).into()
    }

    /// This type with every enum case among its alternatives read as the
    /// whole enum it is one of: `Suit::Hearts|null` is `Suit|null`.
    pub fn enum_cases_as_enums(&self) -> PhpType {
        if let TypeKind::EnumCase(enum_type, _) = self.raw_kind() {
            return enum_type.clone();
        }
        match self.kind() {
            TypeKind::Union(members) if members.iter().any(|m| m.as_enum_case().is_some()) => {
                PhpType::union(members.iter().map(PhpType::enum_cases_as_enums).collect())
            }
            TypeKind::Nullable(inner) if inner.as_enum_case().is_some() => {
                PhpType::nullable(inner.enum_cases_as_enums())
            }
            _ => self.clone(),
        }
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

    /// Union of the members.
    ///
    /// A single member is returned as itself, and repeated alternatives
    /// are dropped (unwrapping the union if one alternative is left),
    /// because a union that names the same type twice is never anything
    /// but noise in a hover or a diagnostic. No other normalisation
    /// happens here — for `true|false` → `bool`, subtype absorption, and
    /// nested-union flattening see [`simplified`](PhpType::simplified).
    pub fn union(mut members: Vec<PhpType>) -> PhpType {
        if members.len() == 1 {
            return members.pop().unwrap();
        }
        if normalize::absorb_non_empty_refinements(&mut members) && members.len() == 1 {
            return members.into_iter().next().unwrap();
        }
        normalize::absorb_int_ranges(&mut members);
        normalize::absorb_subsumed_shapes(&mut members);
        normalize::absorb_exact_values(&mut members);
        if members.len() == 1 {
            return members.into_iter().next().unwrap();
        }
        // The pairwise check spares the small unions nearly every caller
        // builds a hash map, but it is quadratic, so a long union (one
        // literal per `switch` case) goes straight to the hashing dedup.
        if members.len() > 16 || normalize::has_duplicate_members(&members) {
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

    /// Unsealed shape (`array{…, ...<K, V>}`): the entries of `shape`, plus
    /// any number of others keyed `key` and holding `value`.
    ///
    /// A `list{…}` shape makes an unsealed list, whose further entries
    /// continue the count, so `key` is taken to be `int` there. A shape
    /// with no entries says nothing the tail does not, and is returned as
    /// the plain `array<K, V>` / `list<V>`; so is anything that is not a
    /// shape at all, which a transform rebuilding the parts may produce.
    pub fn unsealed_shape(shape: PhpType, key: PhpType, value: PhpType) -> PhpType {
        let is_list = shape.is_list_shape();
        let entries: &[ShapeEntry] = match shape.kind() {
            TypeKind::ArrayShape(entries) => entries,
            _ => &[],
        };
        let non_empty = entries.iter().any(|entry| !entry.optional);
        let widened_value = match shape.iterable_element_type() {
            Some(known) => PhpType::join_runtime_value_types(vec![known, value.clone()]),
            None => value.clone(),
        };
        let widened = if is_list {
            let name = if non_empty { "non-empty-list" } else { "list" };
            PhpType::generic(name, vec![widened_value])
        } else {
            let widened_key = match shape.iterable_key_type() {
                Some(known) => PhpType::join_runtime_value_types(vec![known, key.clone()]),
                None => key.clone(),
            };
            let name = if non_empty {
                "non-empty-array"
            } else {
                "array"
            };
            PhpType::generic(name, vec![widened_key, widened_value])
        };
        if entries.is_empty() {
            return widened;
        }
        TypeKind::UnsealedShape(Box::new(UnsealedShape {
            shape,
            key: if is_list { PhpType::int() } else { key },
            value,
            widened,
        }))
        .into()
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
