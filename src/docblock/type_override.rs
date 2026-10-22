//! Choosing between a docblock type and a native type hint.
//!
//!   - [`should_override_type_typed`]: compatibility check so that a docblock
//!     type only overrides a native type hint when the native hint is broad
//!     enough to be refined.
//!   - [`resolve_effective_type_typed`]: pick the best type between docblock
//!     and native hints.

use std::borrow::Cow;

use crate::php_type::{PhpType, TypeKind};

// ─── Type Override Logic ────────────────────────────────────────────────────

/// Decide whether a docblock type should override a native type hint.
///
/// Returns `true` when the docblock type is likely to carry more
/// information than the native hint (e.g. `Collection<int, User>` vs
/// bare `object`), and `false` when overriding would lose precision
/// (e.g. both are scalars).
pub fn should_override_type_typed(docblock_type: &PhpType, native_type: &PhpType) -> bool {
    // If the docblock type is semantically equivalent to the native type
    // (handles `?X` ↔ `X|null`, reordered unions, FQN vs short names),
    // there is no value in overriding — the docblock doesn't carry any
    // extra information.
    if docblock_type == native_type
        || (docblock_type.equivalent(native_type)
            && class_names_spelled_alike(docblock_type, native_type))
    {
        return false;
    }

    // Unwrap nullable wrappers for further analysis.  `?Foo` → `Foo`,
    // `Foo|null` → `Foo`.  For non-nullable types, use as-is.
    //
    // `non_null_type()` strips nullability from BOTH representations — the
    // `?Foo` (`Nullable`) form and the `Foo|null` (`Union` with a `null`
    // member) form.  Plain `unwrap_nullable()` only handled the former, so a
    // nullable-union native such as `object|null` reached the union branch
    // below with its `null` member still attached.  Since `object` and `null`
    // are both "scalar names", that branch then judged the whole type
    // unrefinable and discarded a generic docblock return like
    // `@psalm-return ?T`, leaving the bare native (`object|null`).
    let doc_owned = docblock_type.non_null_type();
    let doc_inner = doc_owned.as_ref().unwrap_or(docblock_type);
    let native_owned = native_type.non_null_type();
    let native_inner = native_owned.as_ref().unwrap_or(native_type);

    // If the docblock type is a bare, unparameterised primitive scalar
    // (`int`, `string`, `bool`, etc.), there's no value in overriding.
    // We intentionally exclude:
    //  - PHPDoc pseudo-types (`non-empty-string`, `class-string`,
    //    `positive-int`) — these are valid refinements.
    //  - Parameterised types (`array<int>`, `int<0, max>`) — these
    //    carry type information the native hint doesn't have.
    //  - Shapes, callables, slices — these also carry extra info.
    // `true` and `false` are the exception: each is one half of a native
    // `bool`, so `@param true` on `bool $flag` is a refinement like any other.
    if doc_inner.is_bare_primitive_scalar() && !doc_inner.is_true() && !doc_inner.is_false() {
        return false;
    }

    // A type operator the parser could not evaluate — `key-of<TABLE>`,
    // `value-of<TABLE>`, `TABLE[K]` — reads one value out of a table or a
    // template, which no native hint can express: the declaration can only
    // spell the widest type the operator might produce (`int|string` for a
    // table holding both). It refines that hint however it evaluates, so the
    // docblock wins.
    if doc_inner.contains_unevaluated_operator() {
        return true;
    }

    // Produce a lowercased base name for the native type's inner part
    // `array`, `iterable`, `callable`, and `Closure` are broad types
    // that docblocks commonly refine (e.g. `array` → `list<User>`,
    // `iterable` → `Collection<int, Order>`,
    // `callable` → `callable(Task): void`).
    // `is_callable()` covers both `callable` and `Closure` (including `\Closure`).
    if native_inner.is_bare_array() || native_inner.is_iterable() || native_inner.is_callable() {
        return true;
    }

    // If the native type is a union or intersection, check each component.
    // A member is refinable when it is non-scalar (a class the docblock can
    // parameterise) or a broad type (`array`, `iterable`, `callable`,
    // `object`) that a docblock commonly refines. `is_scalar()` counts bare
    // `array` and `object` as scalar, so without the explicit container and
    // object checks a native union like `array|false` would wrongly reject a
    // `array<int, User>|false` docblock (dropping the element type), and
    // `object|string` would reject a `class-string<T>|T` docblock (as with
    // `new ReflectionClass($classString)`, dropping the template binding).
    match native_inner.kind() {
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            if members.iter().any(|m| {
                !m.is_scalar()
                    || m.is_bare_array()
                    || m.is_iterable()
                    || m.is_callable()
                    || m.is_object()
            }) {
                return true;
            }
            // An all-scalar native union can still be refined member by
            // member: `bool|string` → `false|string` is the same `bool` →
            // `false` refinement a lone native `bool` already accepts
            // below, and without it the literal `false` the `!== false`
            // idiom narrows away never reaches the type at all.
            return refines_native_union(doc_inner, members);
        }
        _ => {}
    }

    // If the native type is a narrow scalar (not a broad container
    // handled above), only allow override when the docblock type is a
    // *compatible refinement*.  For example `string` → `class-string<Foo>`
    // is valid, but `string` → `array<int>` is not.
    if native_inner.is_scalar() {
        return is_compatible_refinement_typed(doc_inner, native_inner);
    }

    // If the docblock type carries generic parameters or shape braces,
    // it is refining the class with extra type info — allow it.
    if has_parameterisation(doc_inner) {
        return true;
    }

    // PHPDoc pseudo-types like `class-string`, `non-empty-string`,
    // `positive-int`, `literal-string`, etc. refine their native
    // scalar counterparts.  These contain hyphens which never appear
    // in native PHP types.
    if extract_base_name_lower(doc_inner).contains('-') {
        return true;
    }

    // Native type is a non-scalar class.  Whether a class the docblock
    // names narrows it takes the class hierarchy, so the docblock wins here
    // and loading the declaration checks it (`docblock_narrowing`).
    true
}

/// Whether `a` and `b` name their classes the same way.
///
/// [`PhpType::equivalent`] matches classes by short name, but the parser
/// reconciles a docblock with its native hint before either is qualified, so
/// `ReflectionProperty` may name an imported `Adapter\ReflectionProperty`
/// while the hint spells out the global `\ReflectionProperty`. A docblock
/// that spells a name differently can name a different class, so it is not
/// redundant; when it names the same one, keeping it costs nothing.
fn class_names_spelled_alike(a: &PhpType, b: &PhpType) -> bool {
    let mut a_names = a.extract_class_names();
    let mut b_names = b.extract_class_names();
    if a_names.len() != b_names.len() {
        return false;
    }
    let by_folded_case = |x: &String, y: &String| {
        x.bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(y.bytes().map(|c| c.to_ascii_lowercase()))
    };
    a_names.sort_unstable_by(by_folded_case);
    b_names.sort_unstable_by(by_folded_case);
    a_names
        .iter()
        .zip(&b_names)
        .all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// A declaration docblock with the members removed that its native class
/// type can never hold, or `None` when no member survives.
///
/// A native `DateTimeImmutable` return is what PHP enforces, so the `false`
/// in a `@return static|false` written for an older runtime cannot come
/// back, and neither can the `TValue[]` in `IteratorAggregate::getIterator()`'s
/// `@return Traversable<TKey, TValue>|TValue[]` on a native `Traversable`.
/// PHPStan discards such a docblock outright; keeping the members the
/// native type does allow preserves the generics it carries.  A `null` the
/// native type does not accept goes the same way, as PHPStan also drops it.
fn doc_members_native_can_hold<'a>(doc: &'a PhpType, native: &PhpType) -> Option<Cow<'a, PhpType>> {
    let native_owned = native.non_null_type();
    let native_inner = native_owned.as_ref().unwrap_or(native);
    let native_members: &[PhpType] = match native_inner.kind() {
        TypeKind::Union(members) | TypeKind::Intersection(members) => members,
        _ => std::slice::from_ref(native_inner),
    };
    if !native_members.iter().all(names_a_class) {
        return Some(Cow::Borrowed(doc));
    }
    // A native type that does not accept `null` rules the docblock's `null`
    // out as well: `@return static|null` on `: self` is a `static`.
    let drops_null = !native.accepts_null();
    let can_hold = |m: &PhpType| !is_never_an_object(m) && !(drops_null && m.is_null());
    match doc.kind() {
        TypeKind::Union(members) => {
            let kept: Vec<PhpType> = members.iter().filter(|m| can_hold(m)).cloned().collect();
            if kept.len() == members.len() {
                Some(Cow::Borrowed(doc))
            } else if kept.iter().all(PhpType::is_null) {
                None
            } else {
                Some(Cow::Owned(PhpType::union(kept)))
            }
        }
        TypeKind::Nullable(inner) if drops_null && !is_never_an_object(inner) => {
            Some(Cow::Owned(PhpType::clone(inner)))
        }
        _ if !can_hold(doc) => None,
        _ => Some(Cow::Borrowed(doc)),
    }
}

/// Whether `ty` names one class, interface, or enum: a class name, or the
/// `self` / `static` / `parent` keyword standing for one (not `mixed` or
/// `object`).
fn names_a_class(ty: &PhpType) -> bool {
    let name = match ty.kind() {
        TypeKind::Named(name) => name.as_str(),
        TypeKind::Generic(g) => g.name.as_str(),
        _ => return false,
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "self" | "static" | "parent"
    ) || (crate::php_type::is_class_like_name(name) && !name.eq_ignore_ascii_case("mixed"))
}

/// Whether `ty` is a value that can never be an object (`false`, `int`,
/// `list<Foo>`, `'literal'`, `class-string<Foo>`, …).
///
/// Types that can hold an object (`static`, `$this`, `object`, `callable`,
/// `iterable`, `mixed`, a template or class name) do not count, nor do
/// `null`, `void`, and `never`, which the caller treats separately.
fn is_never_an_object(ty: &PhpType) -> bool {
    fn names_a_non_object(name: &str) -> bool {
        crate::php_type::is_scalar_name_pub(name)
            && !matches!(
                name.to_ascii_lowercase().as_str(),
                "self"
                    | "static"
                    | "parent"
                    | "$this"
                    | "object"
                    | "callable"
                    | "iterable"
                    | "void"
                    | "never"
                    | "null"
            )
    }
    match ty.kind() {
        TypeKind::Named(name) => names_a_non_object(name),
        TypeKind::Generic(g) => names_a_non_object(&g.name),
        TypeKind::Array(_)
        | TypeKind::ArrayShape(_)
        | TypeKind::ClassString(_)
        | TypeKind::InterfaceString(_)
        | TypeKind::IntRange(_, _)
        | TypeKind::Literal(_) => true,
        _ => false,
    }
}

/// Whether a docblock type refines the members of the native union it is
/// written on.
///
/// Every member the docblock names has to line up with a native member it
/// narrows or restates, which is what separates `bool|string` →
/// `false|string` (each member accounted for) from `bool|string` →
/// `array<int>` (a docblock describing something else entirely, where the
/// native hint is the more trustworthy of the two).
fn refines_native_union(doc_type: &PhpType, native_members: &[PhpType]) -> bool {
    let doc_members: &[PhpType] = match doc_type.kind() {
        TypeKind::Union(members) | TypeKind::Intersection(members) => members,
        _ => std::slice::from_ref(doc_type),
    };
    doc_members.iter().all(|doc| {
        native_members
            .iter()
            .any(|native| doc.equivalent(native) || is_compatible_refinement_typed(doc, native))
    })
}

/// Check whether a `PhpType` has generic parameters or shape braces.
fn has_parameterisation(ty: &PhpType) -> bool {
    matches!(
        ty.kind(),
        TypeKind::Generic(_) | TypeKind::ArrayShape(_) | TypeKind::ObjectShape(_)
    )
}

/// Check whether a docblock type is a compatible refinement of a native
/// type.  Both parameters should be stripped of nullable wrappers before
/// calling.
///
/// A refinement is compatible when the docblock's base type narrows the
/// native type without changing its fundamental kind.  For example:
/// - `string` → `class-string<Foo>` (compatible: refines string)
/// - `string` → `non-empty-string` (compatible: refines string)
/// - `int` → `positive-int` (compatible: refines int)
/// - `array` → `list<User>` (compatible: refines array)
/// - `object` → `callable-object` (compatible: refines object)
/// - `string` → `array<int>` (incompatible: completely different type)
/// - `int` → `Collection<User>` (incompatible: completely different type)
///
/// This is the single source of truth for refinement compatibility and
/// is used by both `should_override_type` and the update-docblock
/// contradiction checker.
///
/// Accepts a pre-parsed [`PhpType`] to avoid a parse-stringify-reparse
/// round-trip.  Extracts the outermost type name from the docblock type,
/// stripping generic parameters, shape braces, and callable signatures.
/// Unlike `base_name()` this includes scalar names (`array`, `int`, ...)
/// which are needed for the refinement checks.
pub(crate) fn is_compatible_refinement_typed(doc_type: &PhpType, native_type: &PhpType) -> bool {
    if native_type.is_string_type() {
        return doc_type.is_string_subtype();
    }
    if native_type.is_int() {
        return doc_type.is_int_subtype();
    }
    if native_type.is_float() {
        return doc_type.is_float_subtype();
    }
    if native_type.is_bool() {
        return doc_type.is_bool() || doc_type.is_true() || doc_type.is_false();
    }
    if native_type.is_bare_array() {
        return doc_type.is_array_like() || doc_type.is_iterable();
    }
    if native_type.is_mixed() {
        return true;
    }
    if native_type.is_object() {
        return !doc_type.is_scalar() || matches!(doc_type.kind(), TypeKind::ObjectShape(_));
    }
    if native_type.is_self_like() {
        // `static` and `$this` are valid refinements of `self` — they
        // carry late-static-binding semantics the native `self` lacks.
        if doc_type.is_self_like() {
            return true;
        }
        return !doc_type.is_scalar();
    }
    if native_type.is_void()
        || native_type.is_never()
        || native_type.is_null()
        || native_type.is_true()
        || native_type.is_false()
    {
        return false;
    }
    if native_type.is_callable() {
        return true;
    }

    if native_type.is_iterable() {
        return true;
    }
    if native_type.is_closure() {
        return true;
    }
    if native_type.is_resource() {
        return doc_type.is_resource();
    }
    false
}

/// Extract the outermost type name from a `PhpType` as a lowercased string.
///
/// Strips generic parameters, shape braces, callable signatures, and
/// nullable wrappers.  Returns the base identifier lowercased (e.g.
/// `Generic("Collection", _)` → `"collection"`, `Named("int")` → `"int"`).
///
/// For complex types without a simple base name (e.g. unions, callables,
/// shapes), returns an empty string.
///
/// Used by `should_override_type_typed` for its hyphen-based pseudo-type
/// heuristic.
fn extract_base_name_lower(ty: &PhpType) -> String {
    ty.base_name()
        .map(|n| n.to_ascii_lowercase())
        .unwrap_or_default()
}

// ─── Effective Type Resolution ──────────────────────────────────────────────

/// [`resolve_effective_type_typed`] for a declaration that introduces its
/// own `@template` parameters, `bounds` being their `of` bounds.
///
/// A template name says nothing a native hint can be compared against, but
/// its bound does: `@template T of int` with `@param T $a` on `int $a` is
/// the same `int` the native hint promises, narrowed to whichever `int` the
/// call passes, so the template wins (PHPStan's `decideType` compares a
/// template's bound the same way).  Anything the bound cannot vouch for is
/// judged as before.
pub fn resolve_effective_type_with_template_bounds(
    native_type: Option<&PhpType>,
    docblock_type: Option<&PhpType>,
    bounds: &crate::atom::AtomMap<PhpType>,
) -> Option<PhpType> {
    if let (Some(native), Some(doc)) = (native_type, docblock_type)
        && !bounds.is_empty()
    {
        let names: Vec<crate::atom::Atom> = bounds.keys().copied().collect();
        if doc.references_any_name(&names) {
            let subs: std::collections::HashMap<String, PhpType> = bounds
                .iter()
                .map(|(name, bound)| (name.to_string(), bound.clone()))
                .collect();
            let doc_owned = doc.non_null_type();
            let doc_inner = doc_owned.as_ref().unwrap_or(doc);
            let native_owned = native.non_null_type();
            let native_inner = native_owned.as_ref().unwrap_or(native);
            let resolved = doc_inner.substitute(&subs);
            // A bound the parser could not evaluate (`key-of<Alias>`) is a
            // refinement however it evaluates, as for the docblock itself
            // in `should_override_type_typed`.
            if resolved.contains_unevaluated_operator()
                || resolved.is_subtype_of(native_inner)
                || resolved.equivalent(native_inner)
            {
                return Some(if native.accepts_null() && !native.is_mixed() {
                    doc.clone().or_null()
                } else {
                    doc.clone()
                });
            }
        }
    }
    resolve_effective_type_typed(native_type, docblock_type)
}

/// Pick the best available type between a native type hint and a docblock
/// annotation, returning a parsed [`PhpType`].
///
/// When both are present, the docblock type is used only if
/// [`should_override_type_typed`] approves (i.e. the native hint is broad
/// enough to refine).
///
/// This function does not perform unclosed-bracket recovery; callers with
/// raw docblock strings should use [`super::sanitise_and_parse_docblock_type`]
/// first.
pub fn resolve_effective_type_typed(
    native_type: Option<&PhpType>,
    docblock_type: Option<&PhpType>,
) -> Option<PhpType> {
    match (native_type, docblock_type) {
        // Docblock provided, no native hint → use docblock.
        (None, Some(doc)) => Some(doc.clone()),
        // A declared `never` stands whatever the native hint says: the
        // function does not return, so what it would have returned is moot.
        (Some(_), Some(doc)) if doc.is_never() => Some(doc.clone()),
        // A template bounded by exactly the native type is that type, with
        // the name of the template the value still is.
        (Some(native), Some(doc))
            if doc
                .as_template_param()
                .is_some_and(|(_, bound)| bound.equivalent(native)) =>
        {
            Some(doc.clone())
        }
        // Both present → override only if compatible.
        (Some(native), Some(doc)) => {
            let Some(doc) = doc_members_native_can_hold(doc, native) else {
                return Some(native.clone());
            };
            let doc = doc.as_ref();
            if should_override_type_typed(doc, native) {
                // Preserve nullability from the native hint. A `?array`
                // native with a non-nullable `@param Foo[]` docblock still
                // accepts null at runtime, so the effective type is
                // `Foo[]|null`. This mirrors PHPStan's `decideType`.
                //
                // `mixed` is excluded: it accepts null but carries no
                // explicit null member, so narrowing it through a docblock
                // is intentional and must not re-add null.
                let doc = with_native_members_doc_omits(doc, native);
                // An operator the parser could not evaluate
                // (`Data[value-of<T>]`) has not said yet whether its result
                // takes the `null`; joining one now would outlive the
                // evaluation that decides it.
                if native.accepts_null()
                    && !native.is_mixed()
                    && !doc.contains_unevaluated_operator()
                {
                    Some(doc.or_null())
                } else {
                    Some(doc)
                }
            } else {
                Some(native.clone())
            }
        }
        // Native only → keep it.
        (Some(native), None) => Some(native.clone()),
        // Neither → nothing.
        (None, None) => None,
    }
}

/// `doc` joined with each member of a native union that `doc` says nothing
/// about, the way the native nullability is kept.
///
/// `@param array<int, string>` on `array|false` refines the `array` half
/// and leaves `false` standing, since the value can still be `false` at
/// runtime (PHPStan's `decideType` does the same).  Only keyword types are
/// judged, on both sides: whether a class or a template in the docblock
/// covers a native member takes the class hierarchy or the template's
/// bound, neither of which is known here.
fn with_native_members_doc_omits(doc: &PhpType, native: &PhpType) -> PhpType {
    let TypeKind::Union(native_members) = native.kind() else {
        return doc.clone();
    };
    let doc_members: Vec<PhpType> = match doc.kind() {
        TypeKind::Union(members) => members.to_vec(),
        TypeKind::Nullable(inner) => vec![PhpType::clone(inner), PhpType::null()],
        _ => vec![doc.clone()],
    };
    if !doc_members.iter().all(is_plain_value_type) {
        return doc.clone();
    }
    let omitted: Vec<PhpType> = native_members
        .iter()
        .filter(|member| {
            !member.is_null()
                && !names_a_class(member)
                && !doc_members.iter().any(|d| d.is_subtype_of(member))
        })
        .cloned()
        .collect();
    if omitted.is_empty() {
        return doc.clone();
    }
    let mut members = doc_members.to_vec();
    members.extend(omitted);
    PhpType::union(members)
}

/// Whether `ty` is a keyword, array, or literal type whose relation to a
/// native keyword member can be judged structurally: no class, template,
/// or unevaluated type operator (`CONST[T]`, `key-of<…>`, a conditional)
/// whose meaning depends on something only resolution knows.
fn is_plain_value_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(_) => !names_a_class(ty),
        TypeKind::Generic(g) => !names_a_class(ty) && g.args.iter().all(is_plain_value_type),
        TypeKind::Array(inner) | TypeKind::ListShape(inner) | TypeKind::Nullable(inner) => {
            is_plain_value_type(inner)
        }
        TypeKind::ArrayShape(entries) => entries.iter().all(|e| is_plain_value_type(&e.value_type)),
        TypeKind::Literal(_) | TypeKind::IntRange(..) => true,
        _ => false,
    }
}
