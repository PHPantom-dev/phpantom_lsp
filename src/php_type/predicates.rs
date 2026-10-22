//! Questions about what a type is: its kind, whether it accepts `null`,
//! scalar and literal checks, and the native hint it can be written as.

use super::*;

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

    /// Whether `null` is spelled out in this type: `null` itself, `?T`, or
    /// a union with a `null` member. Unlike [`accepts_null`], `mixed` does
    /// not count.
    ///
    /// [`accepts_null`]: PhpType::accepts_null
    pub fn has_null_member(&self) -> bool {
        match self.kind() {
            TypeKind::Nullable(_) => true,
            TypeKind::Union(members) => members.iter().any(|m| m.has_null_member()),
            _ => self.is_null(),
        }
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
    /// `type_engine::call_resolution::constant_operands`).
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
            TypeKind::Named(s) => {
                with_keyword_lowercase(s, |lower| matches!(lower, "bool" | "boolean"))
            }
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
            TypeKind::Named(s) => {
                with_keyword_lowercase(s, |lower| matches!(lower, "int" | "integer"))
            }
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
                with_keyword_lowercase(s, |lower| matches!(lower, "float" | "double" | "real"))
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
                "string" | string_refinement!() | "view-string"
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
            // `non-empty-lowercase-string&non-falsy-string`: each member
            // narrows the same string.
            TypeKind::Intersection(members) => members.iter().any(|m| m.is_string_subtype()),
            _ => false,
        }
    }

    /// Whether this type demands a string that names a Blade template:
    /// the Laravel PHPStan extensions' `view-string`, on its own or as
    /// the only string-like member of a union.
    ///
    /// `?view-string` and `view-string|null` both count — `null` is not a
    /// string the check could be wrong about — and so does a union with a
    /// non-string alternative such as `view-string|\Illuminate\View\View`,
    /// where a string argument still has to name a template. A union that
    /// also admits a plain `string` does not: the annotation then says
    /// nothing a bare `string` does not already say, so demanding a
    /// template of it would report code the declaration permits.
    pub fn is_view_string(&self) -> bool {
        match self.kind() {
            TypeKind::Named(name) => name.eq_ignore_ascii_case("view-string"),
            TypeKind::Nullable(inner) => inner.is_view_string(),
            TypeKind::Union(members) => {
                members.iter().any(PhpType::is_view_string)
                    && !members
                        .iter()
                        .any(|m| !m.is_view_string() && m.is_string_subtype())
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
            TypeKind::Named(s) => with_keyword_lowercase(s, |lower| {
                matches!(lower, "int" | "integer" | int_refinement!())
            }),
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

    /// This type with every `iterable` alternative spelled out as the
    /// `array|Traversable` union it stands for, type arguments carried
    /// onto both halves (`iterable<K, V>` → `array<K, V>|Traversable<K, V>`).
    ///
    /// A check that tells arrays from objects (`is_array()`,
    /// `instanceof Traversable`) keeps one half and drops the other, which
    /// it cannot do while the two share one name.  Returns `None` when no
    /// top-level alternative is an `iterable`.
    pub fn split_iterable(&self) -> Option<PhpType> {
        let alternatives = match self.kind() {
            TypeKind::Union(members) => &members[..],
            _ => std::slice::from_ref(self),
        };
        fn without_null(member: &PhpType) -> (&PhpType, bool) {
            match member.kind() {
                TypeKind::Nullable(inner) => (inner, true),
                _ => (member, false),
            }
        }
        if !alternatives.iter().any(|m| match without_null(m).0.kind() {
            TypeKind::Named(n) => n.eq_ignore_ascii_case("iterable"),
            TypeKind::Generic(g) => {
                g.name.eq_ignore_ascii_case("iterable") && matches!(g.args.len(), 1 | 2)
            }
            _ => false,
        }) {
            return None;
        }
        let mut members = Vec::with_capacity(alternatives.len() + 1);
        for member in alternatives {
            let (inner, nullable) = without_null(member);
            match inner.split_iterable_member() {
                Some((array, traversable)) => {
                    members.push(array);
                    members.push(traversable);
                }
                None => members.push(inner.clone()),
            }
            if nullable {
                members.push(PhpType::null());
            }
        }
        Some(PhpType::union(members))
    }

    /// The array and `Traversable` halves of a single `iterable` member.
    fn split_iterable_member(&self) -> Option<(PhpType, PhpType)> {
        let traversable = || atom("Traversable");
        match self.kind() {
            TypeKind::Named(n) if n.eq_ignore_ascii_case("iterable") => {
                Some((PhpType::array(), PhpType::named(traversable())))
            }
            TypeKind::Generic(g) if g.name.eq_ignore_ascii_case("iterable") => match &g.args[..] {
                [value] => Some((
                    PhpType::generic_array_val(value.clone()),
                    PhpType::generic_atom(traversable(), vec![PhpType::mixed(), value.clone()]),
                )),
                [key, value] => Some((
                    PhpType::generic_array(key.clone(), value.clone()),
                    PhpType::generic_atom(traversable(), vec![key.clone(), value.clone()]),
                )),
                _ => None,
            },
            _ => None,
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
                    || with_keyword_lowercase(s, |lower| {
                        matches!(
                            lower,
                            "int"
                                | "integer"
                                | "float"
                                | "double"
                                | "real"
                                | "bool"
                                | "boolean"
                                | "true"
                                | "false"
                                | int_refinement!()
                        )
                    })
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
        with_keyword_lowercase(name, |lower| matches!(lower, "array" | "list" | "iterable"))
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
        self.to_native_hint_typed().map(|ty| ty.to_string())
    }

    /// Like [`to_native_hint`] but returns a structured [`PhpType`] instead of a string,
    /// avoiding a parse round-trip.
    pub fn to_native_hint_typed(&self) -> Option<PhpType> {
        match self.raw_kind() {
            TypeKind::Benevolent(inner)
            | TypeKind::ListShape(inner)
            | TypeKind::TemplateParam(_, inner)
            | TypeKind::ClassNameLiteral(inner) => inner.to_native_hint_typed(),
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
            TypeKind::Array(_) | TypeKind::ArrayShape(_) | TypeKind::UnsealedShape(_) => {
                Some(PhpType::array())
            }
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
            TypeKind::Benevolent(inner)
            | TypeKind::ListShape(inner)
            | TypeKind::ClassNameLiteral(inner) => inner.is_informative(),
            TypeKind::Generic(..) | TypeKind::UnsealedShape(..) => true,
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
            TypeKind::StaticType(_) | TypeKind::ThisType(_) | TypeKind::TemplateParam(..) => true,
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
            TypeKind::Benevolent(inner)
            | TypeKind::ListShape(inner)
            | TypeKind::ClassNameLiteral(inner) => {
                inner.references_any_template_param(template_params)
            }
            TypeKind::UnsealedShape(unsealed) => unsealed
                .widened
                .references_any_template_param(template_params),
            TypeKind::Named(name) => template_params.iter().any(|p| p == name),
            TypeKind::TemplateParam(name, bound) => {
                template_params.iter().any(|p| p == name)
                    || bound.references_any_template_param(template_params)
            }
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
