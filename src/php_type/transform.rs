//! Type transformations: name resolution, self-substitution, generics.

use super::*;

/// What `static` / `$this` in a return type bind to once the call they were
/// read for is known.
#[derive(Debug, Clone, Copy)]
enum LsbBinding<'a> {
    /// Bind them over whatever class the replacement type names.
    Inherit,
    /// Bind them over `class`, the class the forwarding call is made from.
    Over(&'a str),
    /// Bind them to `ty`, the receiver's whole statically known type, when
    /// that is richer than a single class name.
    ///
    /// A receiver typed `IfaceA&IfaceB` has a runtime class that satisfies
    /// both, so a method `IfaceA` declares `@return static` returns
    /// something that is still an `IfaceB` as well.  Binding over just the
    /// declaring interface would drop that half.  `self` is unaffected: it
    /// names the class the annotation was read from whatever the receiver
    /// is.
    OverType(&'a PhpType),
    /// Collapse them: the called class is statically fixed, so late static
    /// binding has nothing left to resolve.
    Fixed,
}

impl<'a> LsbBinding<'a> {
    /// The class to bind a `static` / `$this` keyword over, or `None` when the
    /// keyword should collapse to the replacement instead.
    ///
    /// A replacement that is not a plain name (a generic receiver such as
    /// `Builder<Article>`) carries no bound of its own, so `Inherit` collapses
    /// there too and the whole replacement stands in.
    fn bound_over(self, replacement: &PhpType) -> Option<Atom> {
        match self {
            LsbBinding::Inherit => match replacement.kind() {
                TypeKind::Named(name) => Some(*name),
                _ => None,
            },
            LsbBinding::Over(class) => Some(atom(class)),
            LsbBinding::OverType(ty) => match ty.kind() {
                TypeKind::Named(name) => Some(*name),
                _ => None,
            },
            LsbBinding::Fixed => None,
        }
    }

    /// The whole type `static` / `$this` should become, for a binding that
    /// carries more than a class name to bind over.
    fn whole_type(self) -> Option<&'a PhpType> {
        match self {
            LsbBinding::OverType(ty) if !matches!(ty.kind(), TypeKind::Named(_)) => Some(ty),
            _ => None,
        }
    }
}

impl PhpType {
    /// Rebuild this type with `map` applied to each of its immediate
    /// child types, leaving the node's own shape and its non-type data
    /// (shape keys, optionality, parameter flags, a conditional's
    /// parameter and polarity) as they were.
    ///
    /// A leaf has no children and comes back unchanged: a name, a
    /// literal, a raw string, an int range. The nodes that carry a name
    /// beside their children (`Named`, `StaticType`, `ThisType`,
    /// `Generic`, `Callable`) keep it; a walk that rewrites names handles
    /// those arms itself and falls through to here for the rest.
    ///
    /// This is the shared skeleton of every structural rebuild in this
    /// module, so a new `TypeKind` variant only has to be taught to one
    /// walk rather than to seven.
    pub(crate) fn map_children(&self, map: &dyn Fn(&PhpType) -> PhpType) -> PhpType {
        let map_entries = |entries: &[ShapeEntry]| -> Vec<ShapeEntry> {
            entries
                .iter()
                .map(|e| ShapeEntry {
                    key: e.key.clone(),
                    value_type: map(&e.value_type),
                    optional: e.optional,
                })
                .collect()
        };

        match self.raw_kind() {
            TypeKind::Benevolent(inner) => PhpType::benevolent(map(inner)),
            TypeKind::ListShape(inner) => PhpType::as_list_shape(map(inner)),
            TypeKind::Nullable(inner) => PhpType::nullable(map(inner)),
            TypeKind::Union(types) => PhpType::union(types.iter().map(&map).collect()),
            TypeKind::Intersection(types) => {
                PhpType::intersection(types.iter().map(&map).collect())
            }
            TypeKind::Generic(g) => {
                PhpType::generic_atom(g.name, g.args.iter().map(&map).collect())
            }
            TypeKind::Array(inner) => PhpType::array_of(map(inner)),
            TypeKind::ArrayShape(entries) => PhpType::array_shape(map_entries(entries)),
            TypeKind::ObjectShape(entries) => PhpType::object_shape(map_entries(entries)),
            TypeKind::Callable(c) => PhpType::callable_type(CallableType {
                kind: c.kind,
                params: c
                    .params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: map(&p.type_hint),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: c.return_type.as_ref().map(map),
            }),
            TypeKind::Conditional(c) => PhpType::conditional_type(ConditionalType {
                param: c.param,
                negated: c.negated,
                condition: map(&c.condition),
                then_type: map(&c.then_type),
                else_type: map(&c.else_type),
                else_when_undecided: c.else_when_undecided,
            }),
            TypeKind::ClassString(inner) => PhpType::class_string(inner.as_ref().map(map)),
            TypeKind::InterfaceString(inner) => PhpType::interface_string(inner.as_ref().map(map)),
            TypeKind::KeyOf(inner) => PhpType::key_of(map(inner)),
            TypeKind::ValueOf(inner) => PhpType::value_of(map(inner)),
            TypeKind::IndexAccess(target, index) => PhpType::index_access(map(target), map(index)),
            TypeKind::Named(_)
            | TypeKind::StaticType(_)
            | TypeKind::ThisType(_)
            | TypeKind::IntRange(..)
            | TypeKind::Literal(_)
            | TypeKind::Raw(_) => self.clone(),
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
        // A keyword/scalar name is never a class, so it never reaches the
        // caller's resolver.
        let resolve = |name: &Atom| -> Atom {
            if is_keyword_type(name) {
                *name
            } else {
                atom(&resolver(name))
            }
        };
        match self.raw_kind() {
            TypeKind::Named(s) => PhpType::named(resolve(s)),
            TypeKind::Generic(g) => PhpType::generic_atom(
                resolve(&g.name),
                g.args.iter().map(|a| a.resolve_names(resolver)).collect(),
            ),
            TypeKind::Callable(c) => PhpType::callable_type(CallableType {
                kind: resolve(&c.kind),
                params: c
                    .params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.resolve_names(resolver),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: c.return_type.as_ref().map(|rt| rt.resolve_names(resolver)),
            }),
            // `static` and `self` name a class outright rather than
            // maybe-a-keyword, so both go to the resolver unconditionally.
            TypeKind::StaticType(s) => PhpType::static_type(atom(&resolver(s))),
            TypeKind::ThisType(s) => PhpType::this_type(atom(&resolver(s))),
            _ => self.map_children(&|t| t.resolve_names(resolver)),
        }
    }

    /// Return the short (unqualified) name from a potentially
    /// namespace-qualified type name. Returns only the part after the
    /// last `\`. Non-class types pass through unchanged.
    pub(crate) fn short_name_of(name: &str) -> &str {
        crate::util::short_name(name.trim())
    }

    /// Produce a new `PhpType` with all namespace-qualified names
    /// shortened to their unqualified form.
    ///
    /// For example, `App\Models\User|null` becomes `User|null`, and
    /// `array<int, App\Models\User>` becomes `array<int, User>`.
    pub fn shorten(&self) -> PhpType {
        match self.raw_kind() {
            TypeKind::Named(s) => PhpType::named(atom(Self::short_name_of(s))),
            TypeKind::Generic(g) => PhpType::generic(
                Self::short_name_of(&g.name),
                g.args.iter().map(|a| a.shorten()).collect(),
            ),
            TypeKind::Callable(c) => PhpType::callable_type(CallableType {
                kind: atom(Self::short_name_of(&c.kind)),
                params: c
                    .params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.shorten(),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: c.return_type.as_ref().map(|rt| rt.shorten()),
            }),
            TypeKind::StaticType(s) => PhpType::static_type(atom(Self::short_name_of(s))),
            TypeKind::ThisType(s) => PhpType::this_type(atom(Self::short_name_of(s))),
            _ => self.map_children(&|t| t.shorten()),
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
        self.replace_self_with_type(&PhpType::named(atom(class_name)))
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
        self.resolve_self_refs_bounded(class_name, parent_class)
    }

    /// Like [`resolve_self_refs`] but produces bounded static types:
    /// `static` → [`StaticType(bound)`](TypeKind::StaticType),
    /// `$this` → [`ThisType(bound)`](TypeKind::ThisType),
    /// `self` → [`Named(class_name)`](TypeKind::Named),
    /// `parent` → [`Named(parent_class)`](TypeKind::Named).
    ///
    /// Use this when the caller needs to preserve the late-static-binding
    /// distinction rather than flattening everything to a concrete class.
    pub fn resolve_self_refs_bounded(
        &self,
        class_name: &str,
        parent_class: Option<&str>,
    ) -> PhpType {
        match self.raw_kind() {
            TypeKind::Named(s) if is_self_ref_name(s) || s.eq_ignore_ascii_case("parent") => {
                if s.eq_ignore_ascii_case("static") {
                    PhpType::static_type(atom(class_name))
                } else if s.eq_ignore_ascii_case("$this") {
                    PhpType::this_type(atom(class_name))
                } else if s.eq_ignore_ascii_case("parent") {
                    match parent_class {
                        Some(p) => PhpType::named(atom(p)),
                        None => self.clone(),
                    }
                } else {
                    PhpType::named(atom(class_name))
                }
            }
            TypeKind::Generic(g) => {
                let resolved_name = if is_self_ref_name(&g.name) {
                    atom(class_name)
                } else if g.name.eq_ignore_ascii_case("parent") {
                    parent_class.map(atom).unwrap_or(g.name)
                } else {
                    g.name
                };
                PhpType::generic_atom(
                    resolved_name,
                    g.args
                        .iter()
                        .map(|a| a.resolve_self_refs_bounded(class_name, parent_class))
                        .collect(),
                )
            }
            _ => self.map_children(&|t| t.resolve_self_refs_bounded(class_name, parent_class)),
        }
    }

    /// Replace only the `self` keyword (not `static` or `$this`) with a
    /// concrete class name.  Used during inheritance merging so that
    /// inherited methods carry the declaring class's identity for `self`
    /// while preserving `static` for late-static-binding resolution.
    pub fn replace_bare_self(&self, class_name: &str) -> PhpType {
        self.replace_bare_keyword("self", class_name)
    }

    /// Replace only the `parent` keyword with a concrete class name.
    ///
    /// `parent` binds to the parent of the class that *declares* the method,
    /// so an inherited method must carry that class rather than the keyword:
    /// resolving `parent` against the class the call is made on names the
    /// wrong class as soon as another subclass is in between.
    pub fn replace_bare_parent(&self, class_name: &str) -> PhpType {
        self.replace_bare_keyword("parent", class_name)
    }

    fn replace_bare_keyword(&self, keyword: &str, class_name: &str) -> PhpType {
        if let TypeKind::Benevolent(inner) = self.raw_kind() {
            return PhpType::benevolent(inner.replace_bare_keyword(keyword, class_name));
        }
        if let TypeKind::ListShape(inner) = self.raw_kind() {
            return PhpType::as_list_shape(inner.replace_bare_keyword(keyword, class_name));
        }
        match self.kind() {
            TypeKind::Named(s) if s.eq_ignore_ascii_case(keyword) => {
                PhpType::named(atom(class_name))
            }
            TypeKind::Named(_) | TypeKind::Literal(_) | TypeKind::Raw(_) => self.clone(),
            TypeKind::Nullable(inner) => {
                PhpType::nullable(inner.replace_bare_keyword(keyword, class_name))
            }
            TypeKind::Union(types) => PhpType::union(
                types
                    .iter()
                    .map(|t| t.replace_bare_keyword(keyword, class_name))
                    .collect(),
            ),
            TypeKind::Intersection(types) => PhpType::intersection(
                types
                    .iter()
                    .map(|t| t.replace_bare_keyword(keyword, class_name))
                    .collect(),
            ),
            TypeKind::Generic(g) => {
                let resolved_name = if g.name.eq_ignore_ascii_case(keyword) {
                    atom(class_name)
                } else {
                    g.name
                };
                PhpType::generic_atom(
                    resolved_name,
                    g.args
                        .iter()
                        .map(|a| a.replace_bare_keyword(keyword, class_name))
                        .collect(),
                )
            }
            TypeKind::Array(inner) => {
                PhpType::array_of(inner.replace_bare_keyword(keyword, class_name))
            }
            _ => self.clone(),
        }
    }

    /// Returns `true` when this type contains the bare `self` keyword
    /// (not `static` or `$this`).
    pub fn contains_bare_self(&self) -> bool {
        self.contains_bare_keyword("self")
    }

    /// Returns `true` when this type contains the bare `parent` keyword.
    pub fn contains_bare_parent(&self) -> bool {
        self.contains_bare_keyword("parent")
    }

    fn contains_bare_keyword(&self, keyword: &str) -> bool {
        match self.kind() {
            TypeKind::Named(s) => s.eq_ignore_ascii_case(keyword),
            TypeKind::Nullable(inner) => inner.contains_bare_keyword(keyword),
            TypeKind::Union(types) | TypeKind::Intersection(types) => {
                types.iter().any(|t| t.contains_bare_keyword(keyword))
            }
            TypeKind::Generic(g) => {
                g.name.eq_ignore_ascii_case(keyword)
                    || g.args.iter().any(|a| a.contains_bare_keyword(keyword))
            }
            TypeKind::Array(inner) => inner.contains_bare_keyword(keyword),
            _ => false,
        }
    }

    /// Check whether this type tree contains any `self`, `static`, or
    /// `$this` references that [`replace_self`] / [`replace_self_with_type`]
    /// would replace.
    pub fn contains_self_ref(&self) -> bool {
        self.contains_name_matching(&is_self_ref_name)
    }

    /// Check whether this type tree names any of `names`.
    ///
    /// Used to tell a type that still carries a `@template` parameter from
    /// one that is already concrete, so the substitution machinery only runs
    /// when it has something to do.
    pub fn references_any_name(&self, names: &[crate::atom::Atom]) -> bool {
        if names.is_empty() {
            return false;
        }
        self.contains_name_matching(&|name| names.iter().any(|n| n.as_str() == name))
    }

    /// Check whether this type tree contains any relative class-reference
    /// keyword: `self`, `static`, `$this`, or `parent`.
    ///
    /// This is the gate for [`resolve_self_refs_bounded`], which resolves
    /// all four. [`contains_self_ref`] omits `parent`, so using it as the
    /// gate leaves a `parent` type hint unresolved.
    ///
    /// [`resolve_self_refs_bounded`]: PhpType::resolve_self_refs_bounded
    /// [`contains_self_ref`]: PhpType::contains_self_ref
    pub fn contains_relative_class_ref(&self) -> bool {
        self.contains_name_matching(&|name| {
            is_self_ref_name(name) || name.eq_ignore_ascii_case("parent")
        })
    }

    /// Replace every [`TypeKind::Conditional`] in this type tree with the
    /// union of its branches.
    ///
    /// A conditional whose condition has not been decided still describes a
    /// value that satisfies one of its two branches, so `then|else` is the
    /// tightest type that holds however the condition resolves — and, when
    /// both branches are the same type, simply that type.  The raw
    /// conditional is a type *expression*, not a set of values, so anything
    /// that compares a concrete type against it (argument compatibility,
    /// for instance) must collapse it first or nothing will ever match.
    ///
    /// Prefer evaluating the condition against the call's arguments when
    /// they are available; this is the fallback for when they are not.
    pub fn conditionals_as_branch_unions(&self) -> PhpType {
        if !self.contains_conditional() {
            return self.clone();
        }
        let recurse = |inner: &PhpType| inner.conditionals_as_branch_unions();
        match self.raw_kind() {
            TypeKind::Conditional(c) => {
                let mut members: Vec<PhpType> = Vec::new();
                for branch in [&c.then_type, &c.else_type] {
                    for member in recurse(branch).union_members() {
                        if !members.contains(member) {
                            members.push(member.clone());
                        }
                    }
                }
                match members.len() {
                    1 => members.into_iter().next().expect("checked length"),
                    _ => PhpType::union(members),
                }
            }
            _ => self.map_children(&recurse),
        }
    }

    /// Replace every type operator that never got evaluated with the widest
    /// type its result can have: `key-of<T>` with `array-key`, `value-of<T>`
    /// and `T[K]` with `mixed`.
    ///
    /// `key-of<array{a: int}>` is evaluated the moment it is parsed; what
    /// survives is the form whose operand we could not read (a class constant,
    /// a template that was never substituted).  That leftover is a type
    /// *expression*, not a set of values, so anything comparing a concrete type
    /// against it — argument compatibility above all — has to widen it first or
    /// every value, valid or not, comes back as a mismatch.  The bounds are the
    /// ones PHPStan falls back to: an array key is an `int|string` whichever
    /// key it turns out to be, and a value can be anything.
    pub fn unevaluated_operators_as_bounds(&self) -> PhpType {
        if !self.contains_unevaluated_operator() {
            return self.clone();
        }
        let recurse = |inner: &PhpType| inner.unevaluated_operators_as_bounds();
        match self.raw_kind() {
            TypeKind::KeyOf(_) => PhpType::named(atom("array-key")),
            TypeKind::ValueOf(_) | TypeKind::IndexAccess(..) => PhpType::mixed(),
            _ => self.map_children(&recurse),
        }
    }

    /// Walk the type tree looking for a named type whose name satisfies
    /// `pred`.
    fn contains_name_matching(&self, pred: &dyn Fn(&str) -> bool) -> bool {
        match self.raw_kind() {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => {
                inner.contains_name_matching(pred)
            }
            TypeKind::Named(s) => pred(s),
            TypeKind::Nullable(inner) => inner.contains_name_matching(pred),
            TypeKind::Union(types) | TypeKind::Intersection(types) => {
                types.iter().any(|t| t.contains_name_matching(pred))
            }
            TypeKind::Generic(g) => {
                pred(&g.name) || g.args.iter().any(|a| a.contains_name_matching(pred))
            }
            TypeKind::Array(inner) => inner.contains_name_matching(pred),
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => entries
                .iter()
                .any(|e| e.value_type.contains_name_matching(pred)),
            TypeKind::Callable(c) => {
                c.params
                    .iter()
                    .any(|p| p.type_hint.contains_name_matching(pred))
                    || c.return_type
                        .as_ref()
                        .is_some_and(|r| r.contains_name_matching(pred))
            }
            TypeKind::Conditional(c) => {
                c.condition.contains_name_matching(pred)
                    || c.then_type.contains_name_matching(pred)
                    || c.else_type.contains_name_matching(pred)
            }
            TypeKind::ClassString(inner) | TypeKind::InterfaceString(inner) => inner
                .as_ref()
                .is_some_and(|t| t.contains_name_matching(pred)),
            TypeKind::KeyOf(inner) | TypeKind::ValueOf(inner) => inner.contains_name_matching(pred),
            TypeKind::IndexAccess(base, index) => {
                base.contains_name_matching(pred) || index.contains_name_matching(pred)
            }
            TypeKind::StaticType(_) | TypeKind::ThisType(_) => false,
            TypeKind::Literal(_) | TypeKind::Raw(_) | TypeKind::IntRange(_, _) => false,
        }
    }

    /// Replace `self` / `static` / `$this` throughout this type tree
    /// with the given [`PhpType`].
    ///
    /// This is the structured counterpart of [`replace_self`]: instead of
    /// replacing with a bare class name (`PhpType::named(name)`), it
    /// substitutes a full type expression.  This preserves generic
    /// parameters when the receiver is a generic type like
    /// `Builder<Article>`.
    ///
    /// When `replacement` is `TypeKind::Generic("Builder", [Named("Article")])`
    /// and the return type is `Named("static")`, the result is the full
    /// generic type.  When the return type is `Generic("static", [args])`,
    /// the replacement's base name is used and the return type's own args
    /// are kept (they override the receiver's args).
    pub fn replace_self_with_type(&self, replacement: &PhpType) -> PhpType {
        self.replace_self_inner(replacement, LsbBinding::Inherit)
    }

    /// Replace `self` / `static` / `$this` throughout this type tree, with
    /// explicit control over what the late-static-binding keywords bind to.
    ///
    /// `self` always becomes `self_class`, the class whose declaration the
    /// annotation was read from, because `self` is invariant.  `static` and
    /// `$this` become a bounded type over `lsb_class`, the class the call is
    /// made *from*, which is not always the same class: `parent::create()` in
    /// `B extends A` reads the annotation off `A` but still resolves `static`
    /// to `B`.
    ///
    /// Pass `None` for `lsb_class` when the called class is statically fixed —
    /// an explicit `A::create()` on a `static` method, or `new A`.  PHP
    /// resolves `static` to exactly `A` there however `A` is subclassed, so a
    /// bounded [`StaticType`](TypeKind::StaticType) would claim an openness
    /// the call does not have.
    pub fn replace_self_bound(&self, self_class: &str, lsb_class: Option<&str>) -> PhpType {
        let lsb = match lsb_class {
            Some(class) => LsbBinding::Over(class),
            None => LsbBinding::Fixed,
        };
        self.replace_self_inner(&PhpType::named(atom(self_class)), lsb)
    }

    /// Like [`replace_self_bound`](Self::replace_self_bound), but binds
    /// `static` / `$this` to a whole type rather than a single class name.
    ///
    /// For a receiver typed `IfaceA&IfaceB`, late static binding lands on a
    /// runtime class that satisfies both halves, so a `@return static`
    /// declared on `IfaceA` still describes an `IfaceB`.  `self` keeps
    /// naming `self_class`, the class the annotation was read from.
    pub fn replace_self_over_type(&self, self_class: &str, lsb_type: &PhpType) -> PhpType {
        self.replace_self_inner(
            &PhpType::named(atom(self_class)),
            LsbBinding::OverType(lsb_type),
        )
    }

    fn replace_self_inner(&self, replacement: &PhpType, lsb: LsbBinding<'_>) -> PhpType {
        match self.raw_kind() {
            TypeKind::Named(s) if self.is_self_ref() => {
                if !s.eq_ignore_ascii_case("self")
                    && let Some(whole) = lsb.whole_type()
                {
                    return whole.clone();
                }
                let Some(bound) = lsb.bound_over(replacement) else {
                    return replacement.clone();
                };
                if s.eq_ignore_ascii_case("static") {
                    PhpType::static_type(bound)
                } else if s.eq_ignore_ascii_case("$this") {
                    PhpType::this_type(bound)
                } else {
                    replacement.clone()
                }
            }

            TypeKind::Generic(g) if is_self_ref_name(&g.name) => {
                // Only the name part of a generic is replaced, so the
                // replacement contributes its base class name alone.
                let replacement_name = match replacement.kind() {
                    TypeKind::Named(n) | TypeKind::StaticType(n) | TypeKind::ThisType(n) => {
                        n.as_str()
                    }
                    TypeKind::Generic(g) => g.name.as_str(),
                    _ => "",
                };
                PhpType::generic_atom(
                    atom(replacement_name),
                    g.args
                        .iter()
                        .map(|a| a.replace_self_inner(replacement, lsb))
                        .collect(),
                )
            }

            // A bound already applied by an earlier hop still answers to a
            // fixed call target: `A::create()` pins whatever `create()` left
            // open.
            TypeKind::StaticType(n) | TypeKind::ThisType(n) if matches!(lsb, LsbBinding::Fixed) => {
                PhpType::named(*n)
            }

            _ => self.map_children(&|t| t.replace_self_inner(replacement, lsb)),
        }
    }

    /// Substitute template parameter names throughout this type tree.
    ///
    /// Walks the entire type tree and replaces any `Named(s)` node whose
    /// name appears as a key in `subs` with the corresponding `PhpType`.
    /// All other nodes are recursively rebuilt with their children
    /// substituted.
    ///
    /// A `Raw` node — text no type syntax covers, such as the `Foo::BAR`
    /// spelling of a class constant — is looked up the same way. It is only
    /// opaque because nothing has told us what it means; when `subs` does,
    /// that reading wins.
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
        match self.raw_kind() {
            TypeKind::Named(s) => match subs.get(s.as_str()) {
                Some(replacement) => replacement.clone(),
                None => self.clone(),
            },

            // A `Raw` node is text no type syntax covers, so it is only
            // opaque because nothing has said what it means; when `subs`
            // does, that reading wins.
            TypeKind::Raw(s) => match subs.get(s.as_ref()) {
                Some(replacement) => replacement.clone(),
                None => self.clone(),
            },

            TypeKind::Nullable(inner) => {
                let resolved = inner.substitute(subs);
                // A substitution that produced something already nullable
                // must not be wrapped a second time.
                match &resolved.kind() {
                    TypeKind::Nullable(_) => resolved,
                    TypeKind::Union(members) => {
                        if members.iter().any(
                            |m| matches!(m.kind(), TypeKind::Named(n) if n.eq_ignore_ascii_case("null")),
                        ) {
                            resolved
                        } else {
                            PhpType::nullable(resolved)
                        }
                    }
                    _ => PhpType::nullable(resolved),
                }
            }

            // Substitution can put a union inside a union (and an
            // intersection inside an intersection); both flatten back out.
            TypeKind::Union(types) => {
                let mut flat = Vec::with_capacity(types.len());
                for t in types.iter().map(|t| t.substitute(subs)) {
                    match t.kind() {
                        TypeKind::Union(inner) => flat.extend(inner.iter().cloned()),
                        _ => flat.push(t),
                    }
                }
                match flat.len() {
                    1 => flat.into_iter().next().expect("checked length"),
                    _ => PhpType::union(flat),
                }
            }

            TypeKind::Intersection(types) => {
                let mut flat = Vec::with_capacity(types.len());
                for t in types.iter().map(|t| t.substitute(subs)) {
                    match t.kind() {
                        TypeKind::Intersection(inner) => flat.extend(inner.iter().cloned()),
                        _ => flat.push(t),
                    }
                }
                match flat.len() {
                    1 => flat.into_iter().next().expect("checked length"),
                    _ => PhpType::intersection(flat),
                }
            }

            TypeKind::Generic(g) => {
                let args =
                    || -> Vec<PhpType> { g.args.iter().map(|a| a.substitute(subs)).collect() };
                // The base name might itself be a template parameter.
                let Some(replacement) = subs.get(g.name.as_str()) else {
                    return PhpType::generic_atom(g.name, args());
                };
                match replacement.kind() {
                    TypeKind::Named(n) => PhpType::generic_atom(*n, args()),
                    // Keep the replacement's base name but the source
                    // type's own (substituted) args: the replacement's
                    // args describe a different parameterisation.
                    TypeKind::Generic(base) => PhpType::generic_atom(base.name, args()),
                    // For a non-class replacement (a union, an
                    // intersection) the generic wrapper is meaningless.
                    _ => replacement.clone(),
                }
            }

            // Substituting the operand may be what finally makes a type
            // operator evaluable, so each is re-evaluated here.
            TypeKind::KeyOf(inner) => evaluate_key_of(&inner.substitute(subs)),
            TypeKind::ValueOf(inner) => evaluate_value_of(&inner.substitute(subs)),
            TypeKind::IndexAccess(base, index) => {
                evaluate_index_access(&base.substitute(subs), &index.substitute(subs))
            }

            _ => self.map_children(&|t| t.substitute(subs)),
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
    /// Go-to-type-definition uses this rather than [`extract_class_names`]
    /// because it should jump to the container class, not its type
    /// arguments.
    pub fn top_level_class_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_top_level_class_names(&mut names);
        names
    }

    /// Recursive helper for [`extract_class_names`].
    fn collect_class_names(&self, names: &mut Vec<String>) {
        match self.raw_kind() {
            TypeKind::Benevolent(inner) | TypeKind::ListShape(inner) => {
                inner.collect_class_names(names)
            }
            TypeKind::Named(s) => {
                if !is_keyword_type(s) && !s.is_empty() && !names.iter().any(|n| n == s.as_str()) {
                    names.push(s.to_string());
                }
            }

            TypeKind::Nullable(inner) => inner.collect_class_names(names),

            TypeKind::Union(types) | TypeKind::Intersection(types) => {
                for t in types {
                    t.collect_class_names(names);
                }
            }

            TypeKind::Generic(g) => {
                if !is_keyword_type(&g.name)
                    && !g.name.is_empty()
                    && !names.iter().any(|n| n == g.name.as_str())
                {
                    names.push(g.name.to_string());
                }
                for a in &g.args {
                    a.collect_class_names(names);
                }
            }

            TypeKind::Array(inner) => inner.collect_class_names(names),

            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
                for e in entries {
                    e.value_type.collect_class_names(names);
                }
            }

            TypeKind::Callable(c) => {
                for p in &c.params {
                    p.type_hint.collect_class_names(names);
                }
                if let Some(ret) = &c.return_type {
                    ret.collect_class_names(names);
                }
            }

            TypeKind::ClassString(inner) => {
                if let Some(t) = inner {
                    t.collect_class_names(names);
                }
            }

            TypeKind::InterfaceString(inner) => {
                if let Some(t) = inner {
                    t.collect_class_names(names);
                }
            }

            TypeKind::KeyOf(inner) | TypeKind::ValueOf(inner) => {
                inner.collect_class_names(names);
            }

            TypeKind::IndexAccess(base, index) => {
                base.collect_class_names(names);
                index.collect_class_names(names);
            }

            TypeKind::Conditional(c) => {
                c.condition.collect_class_names(names);
                c.then_type.collect_class_names(names);
                c.else_type.collect_class_names(names);
            }

            TypeKind::StaticType(s) | TypeKind::ThisType(s) => {
                if !s.is_empty() && !names.iter().any(|n| n == s.as_str()) {
                    names.push(s.to_string());
                }
            }

            TypeKind::Literal(_) | TypeKind::Raw(_) | TypeKind::IntRange(_, _) => {}
        }
    }

    /// Recursive helper for [`top_level_class_names`].
    ///
    /// Only descends through union, intersection, and nullable
    /// wrappers. Does not recurse into generic args, callable
    /// params/return, shapes, class-string inner types, etc.
    fn collect_top_level_class_names(&self, names: &mut Vec<String>) {
        match self.kind() {
            TypeKind::Named(s)
                if !is_keyword_type(s)
                    && !s.is_empty()
                    && !names.iter().any(|n| n == s.as_str()) =>
            {
                names.push(s.to_string());
            }

            TypeKind::Nullable(inner) => inner.collect_top_level_class_names(names),

            TypeKind::Union(types) | TypeKind::Intersection(types) => {
                for t in types {
                    t.collect_top_level_class_names(names);
                }
            }

            // For generics, only the base name is top-level.
            // `Collection<int, User>` → `["Collection"]`.
            TypeKind::Generic(g)
                if !is_keyword_type(&g.name)
                    && !g.name.is_empty()
                    && !names.iter().any(|n| n == g.name.as_str()) =>
            {
                names.push(g.name.to_string());
            }

            TypeKind::StaticType(s) | TypeKind::ThisType(s)
                if !s.is_empty() && !names.iter().any(|n| n == s.as_str()) =>
            {
                names.push(s.to_string());
            }

            // `User[]` — the inner type is the top-level class.
            TypeKind::Array(inner) => inner.collect_top_level_class_names(names),

            // Shapes, callables, class-string, key-of, value-of,
            // conditionals, literals, int-ranges — no navigable
            // top-level class name.
            _ => {}
        }
    }
}
