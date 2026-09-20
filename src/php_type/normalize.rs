//! Union/intersection simplification and normalization.

use std::collections::{
    HashMap, HashSet,
    hash_map::{DefaultHasher, Entry},
};
use std::hash::{Hash, Hasher};

use super::*;

impl PhpType {
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
        if let TypeKind::Benevolent(inner) = self.raw_kind() {
            return PhpType::benevolent(inner.simplified());
        }
        match self.kind() {
            TypeKind::Union(members) => {
                let mut simplified: Vec<PhpType> = Vec::with_capacity(members.len());
                for m in members {
                    let s = m.simplified();
                    // Flatten nested unions.
                    if let TypeKind::Union(inner) = s.kind() {
                        simplified.extend(inner.iter().cloned());
                    } else {
                        simplified.push(s);
                    }
                }

                if simplified.iter().any(|m| m.is_mixed()) {
                    return PhpType::mixed();
                }

                // Deduplicate without folding case-sensitive literal payloads.
                dedup_types(&mut simplified);

                simplify_bool_union(&mut simplified);
                absorb_scalar_refinements(&mut simplified);

                if simplified.len() == 1 {
                    return simplified.into_iter().next().unwrap();
                }
                if simplified.is_empty() {
                    return PhpType::never();
                }

                PhpType::union(simplified)
            }
            TypeKind::Intersection(members) => {
                let mut simplified: Vec<PhpType> = Vec::with_capacity(members.len());
                for m in members {
                    let s = m.simplified();
                    // Flatten nested intersections.
                    if let TypeKind::Intersection(inner) = s.kind() {
                        simplified.extend(inner.iter().cloned());
                    } else {
                        simplified.push(s);
                    }
                }

                dedup_types(&mut simplified);

                if simplified.iter().any(|m| m.is_never()) {
                    return PhpType::never();
                }

                if simplified.len() == 1 {
                    return simplified.into_iter().next().unwrap();
                }
                if simplified.is_empty() {
                    return PhpType::mixed();
                }

                PhpType::intersection(simplified)
            }
            TypeKind::Nullable(inner) => {
                let s = inner.simplified();
                if s.is_never() || s.is_null() {
                    PhpType::null()
                } else if s.is_mixed() {
                    PhpType::mixed()
                } else {
                    PhpType::nullable(s)
                }
            }
            TypeKind::Generic(g) => {
                let simplified_args: Vec<PhpType> = g.args.iter().map(|a| a.simplified()).collect();
                PhpType::generic_atom(g.name, simplified_args)
            }
            TypeKind::Array(inner) => PhpType::array_of(inner.simplified()),
            TypeKind::ClassString(inner) => {
                PhpType::class_string(inner.as_ref().map(|i| i.simplified()))
            }
            TypeKind::InterfaceString(inner) => {
                PhpType::interface_string(inner.as_ref().map(|i| i.simplified()))
            }
            TypeKind::KeyOf(inner) => PhpType::key_of(inner.simplified()),
            TypeKind::ValueOf(inner) => PhpType::value_of(inner.simplified()),
            // Leaf types are already simplified.
            _ => self.clone(),
        }
    }

    /// Widen scalar literal values at a mutable-collection boundary.
    ///
    /// Literal members inside unions and nullable wrappers are widened as
    /// well, then duplicate alternatives are removed. Structural payloads
    /// such as generic arguments, callable signatures, and existing shapes
    /// are deliberately left untouched: storing `Box<'draft'>` in an array
    /// must not rewrite the type carried by `Box`.
    ///
    /// `true` and `false` are not widened. Unlike `'draft'`, which stands
    /// for one value out of an open domain, a boolean half names one of the
    /// only two values there are, and it is how PHP's failure-signal unions
    /// (`realpath(): string|false`) are spelled. Widening it to `bool` adds
    /// an alternative the expression can never produce, which a later
    /// `!false` assertion then leaves behind as a phantom `true`.
    #[must_use]
    pub(crate) fn widen_scalar_literals(&self) -> PhpType {
        if let TypeKind::Benevolent(inner) = self.raw_kind() {
            return PhpType::benevolent(inner.widen_scalar_literals());
        }
        match self.kind() {
            TypeKind::Literal(value) => match &**value {
                LiteralValue::Int(_) => PhpType::int(),
                LiteralValue::Float(_) => PhpType::float(),
                LiteralValue::String(_) => PhpType::string(),
            },
            TypeKind::Union(members) => {
                let mut widened: Vec<PhpType> = Vec::with_capacity(members.len());
                let mut seen = HashSet::with_capacity(members.len());
                for member in members {
                    let member = member.widen_scalar_literals();
                    // Recursive widening can expose an existing nested union.
                    // Flatten only this top-level value set; do not recurse
                    // into generic, callable, or shape payloads.
                    for alternative in member.union_members() {
                        if seen.insert(alternative.clone()) {
                            widened.push(alternative.clone());
                        }
                    }
                }
                if widened.len() == 1 {
                    widened.into_iter().next().unwrap()
                } else {
                    PhpType::union(widened)
                }
            }
            TypeKind::Nullable(inner) => PhpType::nullable(inner.widen_scalar_literals()),
            _ => self.clone(),
        }
    }

    /// Replace the `true` and `false` halves of this type with `bool`.
    ///
    /// `true` and `false` are types in their own right from PHP 8.2 on, and
    /// the type engine tracks them so a truthiness check has something to
    /// subtract. A declaration written back into source is a different
    /// matter: `bool` is what the author would have typed, it does not
    /// commit the signature to one half, and it parses on every supported
    /// PHP version.
    #[must_use]
    pub(crate) fn widen_boolean_literals(&self) -> PhpType {
        if let TypeKind::Benevolent(inner) = self.raw_kind() {
            return PhpType::benevolent(inner.widen_boolean_literals());
        }
        match self.kind() {
            TypeKind::Named(name)
                if name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("false") =>
            {
                PhpType::bool()
            }
            TypeKind::Union(members) => PhpType::union(
                members
                    .iter()
                    .map(PhpType::widen_boolean_literals)
                    .collect(),
            ),
            TypeKind::Nullable(inner) => PhpType::nullable(inner.widen_boolean_literals()),
            _ => self.clone(),
        }
    }

    /// Drop the alternatives of this type that a native declaration of
    /// `native` forbids.
    ///
    /// An implementation without its own docblock inherits the interface's
    /// `@return`, but its own native hint is a promise the interface's wider
    /// union does not get to override: an interface declaring
    /// `@return array<string, mixed>|list<mixed>|string` and an
    /// implementation declaring `: array` can only ever return an array.
    ///
    /// Only alternatives whose value domain is unmistakable take part. Class
    /// names, templates, `object`, `callable`, and `iterable` all straddle
    /// several domains (a class can be `Traversable`, a string can be
    /// callable), so anything that cannot be classified on both sides is
    /// kept — the result is never narrower than what can be proved.
    #[must_use]
    pub(crate) fn without_alternatives_the_native_type_forbids(&self, native: &PhpType) -> PhpType {
        let TypeKind::Union(members) = self.kind() else {
            return self.clone();
        };
        let Some(allowed) = native_value_domains(native) else {
            return self.clone();
        };

        let kept: Vec<PhpType> = members
            .iter()
            .filter(|member| match value_domain(member) {
                Some(domain) => allowed.contains(&domain),
                None => true,
            })
            .cloned()
            .collect();

        match kept.len() {
            0 => self.clone(),
            1 => kept.into_iter().next().unwrap(),
            _ => PhpType::union(kept),
        }
    }

    /// Join alternatives that can be produced at runtime.
    ///
    /// This is intentionally separate from [`PhpType::simplified`], whose
    /// subtype lattice also models parameter coercions such as `int <: float`.
    /// A runtime expression may still produce either an int or a float, so
    /// those domains must remain distinct while true value-set subsets such
    /// as `1 <: int`, `1 <: numeric`, and `'x' <: scalar` are absorbed.
    pub(crate) fn join_runtime_value_types(members: Vec<PhpType>) -> PhpType {
        fn flatten(member: PhpType, flattened: &mut Vec<PhpType>) {
            match member.kind() {
                TypeKind::Union(members) => {
                    for member in members.iter().cloned() {
                        flatten(member, flattened);
                    }
                }
                TypeKind::Nullable(inner) => {
                    flatten(inner.clone(), flattened);
                    flattened.push(PhpType::null());
                }
                _ => flattened.push(member),
            }
        }

        let mut flattened = Vec::with_capacity(members.len());
        for member in members {
            flatten(member, &mut flattened);
        }
        if flattened.iter().any(PhpType::is_mixed) {
            return PhpType::mixed();
        }

        flattened.retain(|member| !member.is_never());
        dedup_types(&mut flattened);
        simplify_bool_union(&mut flattened);

        // Branch joins are normally tiny. Pairwise containment keeps the
        // semantics explicit and handles equivalent aliases deterministically
        // without putting subtype policy into the global dedup key.
        let mut keep = vec![true; flattened.len()];
        for index in 0..flattened.len() {
            for candidate in 0..flattened.len() {
                if index == candidate
                    || !is_runtime_value_subtype(&flattened[index], &flattened[candidate])
                {
                    continue;
                }

                let equivalent = is_runtime_value_subtype(&flattened[candidate], &flattened[index]);
                if !equivalent || candidate < index {
                    keep[index] = false;
                    break;
                }
            }
        }

        crate::util::retain_by_mask(&mut flattened, &keep);

        match flattened.len() {
            0 => PhpType::never(),
            1 => flattened.into_iter().next().unwrap(),
            _ => PhpType::union(flattened),
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
        match self.kind() {
            TypeKind::Intersection(members) => {
                let has_union = members
                    .iter()
                    .any(|m| matches!(m.kind(), TypeKind::Union(_)));
                if !has_union {
                    return self.clone();
                }

                // Collect each member as a list of alternatives.
                // Non-union members are singleton lists.
                let alternatives: Vec<Vec<PhpType>> = members
                    .iter()
                    .map(|m| match m.kind() {
                        TypeKind::Union(u) => u.to_vec(),
                        _ => vec![m.clone()],
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
                            PhpType::intersection(combo)
                        }
                    })
                    .collect();

                if union_members.len() == 1 {
                    union_members.into_iter().next().unwrap().simplified()
                } else {
                    PhpType::union(union_members).simplified()
                }
            }
            _ => self.clone(),
        }
    }
}

/// Whether `subtype`'s produced values are a subset of `supertype`'s, so a
/// join of the two can drop `subtype` without losing an alternative.
///
/// Only scalar value domains take part; see [`is_runtime_scalar_value_domain`].
pub(crate) fn is_runtime_value_subtype(subtype: &PhpType, supertype: &PhpType) -> bool {
    // `array{}` is the empty array, and every array type that does not
    // demand an entry has it as a member value.  That is real value
    // containment rather than the variance/coercion kind
    // [`is_runtime_scalar_value_domain`] rules out, so a branch that only
    // produced `[]` adds nothing beside a branch that produced an array of
    // the same family — which is what keeps the `[]` a loop or a by-ref
    // closure capture starts from out of the joined result.
    if subtype.is_empty_array_shape() && supertype.accepts_empty_array() {
        return true;
    }

    // `flatten` keeps a union supertype atomic when it is wrapped in a
    // marker (`Benevolent`'s failure branch has to survive on whatever
    // entry represents it — see `ResolvedType::types_joined`), so a
    // `Benevolent(int|float)` reaches here as one member rather than
    // decomposed into `int` and `float`.  A sibling that only duplicates
    // one of its members is still redundant, so check containment against
    // each member without decomposing (and so without disturbing) the
    // union itself.
    if let TypeKind::Union(members) = supertype.kind() {
        return members.iter().any(|m| is_runtime_value_subtype(subtype, m));
    }

    if !is_runtime_scalar_value_domain(subtype) || !is_runtime_scalar_value_domain(supertype) {
        return false;
    }

    if let (TypeKind::Named(sub), TypeKind::Named(sup)) = (subtype.kind(), supertype.kind())
        && sub.eq_ignore_ascii_case("number")
        && sup.eq_ignore_ascii_case("number")
        && (sub == "number") != (sup == "number")
    {
        // Lowercase `number` is PHPantom's numeric pseudo-type, while another
        // casing may be a real class (for example `BcMath\Number`).
        return false;
    }

    // PHP accepts an int for a float parameter, but the runtime value remains
    // an int. This is the coercive edge that must not participate in a
    // produced-value join.
    if subtype.is_int_subtype() && supertype.is_float_subtype() {
        return false;
    }
    subtype.is_subtype_of(supertype)
}

/// Whether a type is a scalar runtime value domain whose subtype edges describe
/// value-set containment rather than structural or parameter coercion.
///
/// Runtime branch joining must not apply the full subtype lattice to generic
/// arrays, callables, shapes, or other structured alternatives: their subtype
/// relations can be variance/coercion rules rather than proof that one branch's
/// produced values are redundant.
fn is_runtime_scalar_value_domain(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Literal(_)
        | TypeKind::IntRange(_, _)
        | TypeKind::ClassString(_)
        | TypeKind::InterfaceString(_) => true,
        TypeKind::Named(name) => {
            if name == "number" {
                return true;
            }
            matches!(
                keyword_lowercase(name).as_str(),
                "int"
                    | "integer"
                    | "positive-int"
                    | "negative-int"
                    | "non-positive-int"
                    | "non-negative-int"
                    | "non-zero-int"
                    | "float"
                    | "double"
                    | "real"
                    | "string"
                    | "non-empty-string"
                    | "non-empty-lowercase-string"
                    | "lowercase-string"
                    | "uppercase-string"
                    | "non-empty-uppercase-string"
                    | "truthy-string"
                    | "non-falsy-string"
                    | "literal-string"
                    | "non-empty-literal-string"
                    | "numeric-string"
                    | "callable-string"
                    | "class-string"
                    | "interface-string"
                    | "trait-string"
                    | "enum-string"
                    | "bool"
                    | "boolean"
                    | "true"
                    | "false"
                    | "null"
                    | "numeric"
                    | "scalar"
                    | "array-key"
            )
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Simplification helpers (private)
// ---------------------------------------------------------------------------

/// A runtime value domain that no other domain overlaps.
///
/// Deliberately excludes `object`, `callable`, `iterable`, and `resource`:
/// each of those admits values from more than one domain (a `callable` may
/// be a string, an array, or an object), so a type naming one proves
/// nothing about which domain a sibling alternative belongs to.
#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum ValueDomain {
    Array,
    String,
    Int,
    Float,
    Bool,
    Null,
}

/// The domains a native declaration of `ty` admits, or `None` when it
/// admits values this classification cannot pin down.
fn native_value_domains(ty: &PhpType) -> Option<Vec<ValueDomain>> {
    match ty.kind() {
        TypeKind::Nullable(inner) => {
            let mut domains = native_value_domains(inner)?;
            domains.push(ValueDomain::Null);
            Some(domains)
        }
        TypeKind::Union(members) => {
            let mut domains = Vec::with_capacity(members.len());
            for member in members {
                domains.extend(native_value_domains(member)?);
            }
            Some(domains)
        }
        _ => {
            let mut domains = vec![value_domain(ty)?];
            // A native `float` accepts an `int` return and coerces it, so a
            // docblock naming `int` beside it is describing what the body
            // produces rather than contradicting the declaration.
            if domains == [ValueDomain::Float] {
                domains.push(ValueDomain::Int);
            }
            Some(domains)
        }
    }
}

/// The single domain every value of `ty` belongs to, or `None` when `ty`
/// spans several (or names something this cannot classify).
fn value_domain(ty: &PhpType) -> Option<ValueDomain> {
    match ty.kind() {
        TypeKind::Array(_) | TypeKind::ArrayShape(_) | TypeKind::ListShape(_) => {
            Some(ValueDomain::Array)
        }
        TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => Some(ValueDomain::String),
        TypeKind::IntRange(_, _) => Some(ValueDomain::Int),
        TypeKind::Literal(value) => Some(match &**value {
            LiteralValue::Int(_) => ValueDomain::Int,
            LiteralValue::Float(_) => ValueDomain::Float,
            LiteralValue::String(_) => ValueDomain::String,
        }),
        TypeKind::Generic(generic) => named_value_domain(&generic.name),
        TypeKind::Named(name) => named_value_domain(name),
        _ => None,
    }
}

/// The domain a keyword type name belongs to, or `None` for class names,
/// templates, and the keywords that straddle domains.
fn named_value_domain(name: &str) -> Option<ValueDomain> {
    match native_scalar_name(name)? {
        "array" => Some(ValueDomain::Array),
        "string" => Some(ValueDomain::String),
        "int" => Some(ValueDomain::Int),
        "float" => Some(ValueDomain::Float),
        "bool" | "true" | "false" => Some(ValueDomain::Bool),
        "null" => Some(ValueDomain::Null),
        _ => None,
    }
}

/// Whether `types` names the same alternative more than once.
///
/// [`dedup_types`] allocates a hash table, which is wasted work on the
/// overwhelmingly common case of a union that is already distinct. Unions
/// are small (two or three members in almost every real type), so a
/// pairwise scan answers the question without touching the allocator.
pub(crate) fn has_duplicate_members(types: &[PhpType]) -> bool {
    types.iter().enumerate().skip(1).any(|(index, ty)| {
        types[..index]
            .iter()
            .any(|seen| equivalent_for_dedup(seen, ty))
    })
}

/// Deduplicate types while treating identifiers, but not literal payloads, as
/// case-insensitive.
///
/// PHP type and class names are case-insensitive, whereas string literal values
/// and shape keys are not. Lower-casing the complete display form would merge
/// distinct types such as `'A'|'a'` and `Box<'A'>|Box<'a'>`.
pub(crate) fn dedup_types(types: &mut Vec<PhpType>) {
    enum DedupBucket {
        One(PhpType),
        Many(Box<[PhpType]>),
    }

    impl DedupBucket {
        fn contains(&self, ty: &PhpType) -> bool {
            match self {
                Self::One(existing) => equivalent_for_dedup(existing, ty),
                Self::Many(existing) => existing
                    .iter()
                    .any(|existing| equivalent_for_dedup(existing, ty)),
            }
        }

        fn push_collision(&mut self, ty: PhpType) {
            match self {
                Self::One(existing) => {
                    *self = Self::Many(vec![existing.clone(), ty].into_boxed_slice());
                }
                Self::Many(existing) => {
                    let mut expanded = existing.to_vec();
                    expanded.push(ty);
                    *existing = expanded.into_boxed_slice();
                }
            }
        }
    }

    // Keep expected complexity linear without trusting hashes for equality:
    // the inline singleton makes the common path allocation-free, while an
    // actual hash collision spills to a boxed slice for explicit comparison.
    let mut buckets: HashMap<u64, DedupBucket> = HashMap::with_capacity(types.len());
    types.retain(|ty| {
        let mut hasher = DefaultHasher::new();
        hash_for_dedup(ty, &mut hasher);
        match buckets.entry(hasher.finish()) {
            Entry::Vacant(entry) => {
                entry.insert(DedupBucket::One(ty.clone()));
                true
            }
            Entry::Occupied(mut entry) => {
                if entry.get().contains(ty) {
                    false
                } else {
                    entry.get_mut().push_collision(ty.clone());
                    true
                }
            }
        }
    });
}

fn named_identifiers_equivalent(left: &str, right: &str) -> bool {
    if left.eq_ignore_ascii_case("number") && right.eq_ignore_ascii_case("number") {
        // Only the exact lowercase spelling is PHPantom's `number`
        // pseudo-type. Other casing may denote a real class such as
        // `BcMath\Number`.
        return (left == "number") == (right == "number");
    }
    left.eq_ignore_ascii_case(right)
}

fn identifiers_equivalent(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn hash_identifier<H: Hasher>(value: &str, state: &mut H) {
    for byte in value.bytes() {
        state.write_u8(byte.to_ascii_lowercase());
    }
}

fn hash_named_identifier<H: Hasher>(value: &str, state: &mut H) {
    let is_number_pseudo_type = value == "number";
    is_number_pseudo_type.hash(state);
    hash_identifier(value, state);
}

fn hash_for_dedup<H: Hasher>(ty: &PhpType, state: &mut H) {
    std::mem::discriminant(ty.kind()).hash(state);
    match ty.kind() {
        TypeKind::Named(name) => hash_named_identifier(name, state),
        TypeKind::StaticType(name) | TypeKind::ThisType(name) => hash_identifier(name, state),
        TypeKind::Nullable(inner)
        | TypeKind::Array(inner)
        | TypeKind::KeyOf(inner)
        | TypeKind::ValueOf(inner) => hash_for_dedup(inner, state),
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.len().hash(state);
            for member in members {
                hash_for_dedup(member, state);
            }
        }
        TypeKind::Generic(generic) => {
            hash_identifier(&generic.name, state);
            generic.args.len().hash(state);
            for argument in &generic.args {
                hash_for_dedup(argument, state);
            }
        }
        TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
            entries.len().hash(state);
            for entry in entries {
                entry.key.hash(state);
                entry.optional.hash(state);
                hash_for_dedup(&entry.value_type, state);
            }
        }
        TypeKind::Callable(callable) => {
            hash_identifier(&callable.kind, state);
            callable.params.len().hash(state);
            for parameter in &callable.params {
                parameter.optional.hash(state);
                parameter.variadic.hash(state);
                hash_for_dedup(&parameter.type_hint, state);
            }
            callable.return_type.is_some().hash(state);
            if let Some(return_type) = &callable.return_type {
                hash_for_dedup(return_type, state);
            }
        }
        TypeKind::Conditional(conditional) => {
            conditional.param.hash(state);
            conditional.negated.hash(state);
            hash_for_dedup(&conditional.condition, state);
            hash_for_dedup(&conditional.then_type, state);
            hash_for_dedup(&conditional.else_type, state);
        }
        TypeKind::ClassString(inner) | TypeKind::InterfaceString(inner) => {
            inner.is_some().hash(state);
            if let Some(inner) = inner {
                hash_for_dedup(inner, state);
            }
        }
        TypeKind::IndexAccess(key, value) => {
            hash_for_dedup(key, state);
            hash_for_dedup(value, state);
        }
        TypeKind::Literal(value) => match &**value {
            LiteralValue::Int(raw) => {
                0_u8.hash(state);
                match value.parse_i64() {
                    Some(parsed) => {
                        0_u8.hash(state);
                        parsed.hash(state);
                    }
                    None => {
                        1_u8.hash(state);
                        raw.hash(state);
                    }
                }
            }
            LiteralValue::Float(raw) => {
                1_u8.hash(state);
                match value.parse_f64() {
                    Some(parsed) if parsed.is_nan() => {
                        0_u8.hash(state);
                        raw.hash(state);
                    }
                    Some(parsed) => {
                        1_u8.hash(state);
                        let bits = if parsed == 0.0 { 0 } else { parsed.to_bits() };
                        bits.hash(state);
                    }
                    None => {
                        2_u8.hash(state);
                        raw.hash(state);
                    }
                }
            }
            LiteralValue::String(raw) => {
                2_u8.hash(state);
                match value.string_content() {
                    Some(content) => {
                        0_u8.hash(state);
                        content.hash(state);
                    }
                    None => {
                        1_u8.hash(state);
                        raw.hash(state);
                    }
                }
            }
        },
        // Exact structural equality is pointer equality because PhpType is
        // interned. Identity therefore hashes every remaining equivalence
        // class consistently.
        _ => ty.identity().hash(state),
    }
}

fn equivalent_for_dedup(left: &PhpType, right: &PhpType) -> bool {
    if left == right {
        return true;
    }

    match (left.kind(), right.kind()) {
        (TypeKind::Named(a), TypeKind::Named(b)) => named_identifiers_equivalent(a, b),
        (TypeKind::StaticType(a), TypeKind::StaticType(b))
        | (TypeKind::ThisType(a), TypeKind::ThisType(b)) => identifiers_equivalent(a, b),
        (TypeKind::Nullable(a), TypeKind::Nullable(b))
        | (TypeKind::Array(a), TypeKind::Array(b))
        | (TypeKind::KeyOf(a), TypeKind::KeyOf(b))
        | (TypeKind::ValueOf(a), TypeKind::ValueOf(b)) => equivalent_for_dedup(a, b),
        (TypeKind::Union(a), TypeKind::Union(b))
        | (TypeKind::Intersection(a), TypeKind::Intersection(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(a, b)| equivalent_for_dedup(a, b))
        }
        (TypeKind::Generic(a), TypeKind::Generic(b)) => {
            identifiers_equivalent(&a.name, &b.name)
                && a.args.len() == b.args.len()
                && a.args
                    .iter()
                    .zip(b.args.iter())
                    .all(|(a, b)| equivalent_for_dedup(a, b))
        }
        (TypeKind::ArrayShape(a), TypeKind::ArrayShape(b))
        | (TypeKind::ObjectShape(a), TypeKind::ObjectShape(b)) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|(a, b)| {
                    a.key == b.key
                        && a.optional == b.optional
                        && equivalent_for_dedup(&a.value_type, &b.value_type)
                })
        }
        (TypeKind::Callable(a), TypeKind::Callable(b)) => {
            identifiers_equivalent(&a.kind, &b.kind)
                && a.params.len() == b.params.len()
                && a.params.iter().zip(b.params.iter()).all(|(a, b)| {
                    a.optional == b.optional
                        && a.variadic == b.variadic
                        && equivalent_for_dedup(&a.type_hint, &b.type_hint)
                })
                && match (&a.return_type, &b.return_type) {
                    (Some(a), Some(b)) => equivalent_for_dedup(a, b),
                    (None, None) => true,
                    _ => false,
                }
        }
        (TypeKind::Conditional(a), TypeKind::Conditional(b)) => {
            a.param == b.param
                && a.negated == b.negated
                && equivalent_for_dedup(&a.condition, &b.condition)
                && equivalent_for_dedup(&a.then_type, &b.then_type)
                && equivalent_for_dedup(&a.else_type, &b.else_type)
        }
        (TypeKind::ClassString(a), TypeKind::ClassString(b))
        | (TypeKind::InterfaceString(a), TypeKind::InterfaceString(b)) => match (a, b) {
            (Some(a), Some(b)) => equivalent_for_dedup(a, b),
            (None, None) => true,
            _ => false,
        },
        (TypeKind::IndexAccess(a_key, a_value), TypeKind::IndexAccess(b_key, b_value)) => {
            equivalent_for_dedup(a_key, b_key) && equivalent_for_dedup(a_value, b_value)
        }
        (TypeKind::Literal(a), TypeKind::Literal(b)) => literals_equal(a, b),
        _ => false,
    }
}

/// If a union contains both `true` and `false`, replace them with `bool`.
pub(crate) fn simplify_bool_union(types: &mut Vec<PhpType>) {
    let has_true = types
        .iter()
        .any(|t| matches!(t.kind(), TypeKind::Named(s) if s.eq_ignore_ascii_case("true")));
    let has_false = types
        .iter()
        .any(|t| matches!(t.kind(), TypeKind::Named(s) if s.eq_ignore_ascii_case("false")));

    if has_true && has_false {
        types.retain(|t| {
            !matches!(t.kind(), TypeKind::Named(s)
                if matches!(s.to_ascii_lowercase().as_str(), "true" | "false"))
        });
        types.push(PhpType::bool());
    }
}

/// Drop every `non-empty-` refinement whose unrefined base shares the
/// union, reporting whether anything was removed.
///
/// A truthy narrowing contributes `non-empty-list<Item>` where the
/// alternative beside it is the plain `list<Item>` it was refined from
/// (`$items = $this->getItems() ?: $this->getDefaultItems()`), and a
/// union spelling both reads as if they were different types. Unlike
/// [`absorb_scalar_refinements`], which compares plain names, this also
/// pairs the parameterised spellings, whose arguments must match for the
/// wider member to cover the narrower one.
pub(crate) fn absorb_non_empty_refinements(types: &mut Vec<PhpType>) -> bool {
    if types.len() < 2 || !types.iter().any(is_non_empty_refinement) {
        return false;
    }

    let mut changed = absorb_empty_shape_into_non_empty_array(types);
    if types.len() < 2 {
        return true;
    }

    let keep: Vec<bool> = types
        .iter()
        .map(|ty| match unrefined_base(ty) {
            // The refinement itself never matches its own base, so no
            // index bookkeeping is needed to skip it.
            Some(base) => !types.iter().any(|other| equivalent_for_dedup(other, &base)),
            None => true,
        })
        .collect();

    let before = types.len();
    crate::util::retain_by_mask(types, &keep);
    changed |= types.len() != before;
    changed
}

/// Widen `array{} | non-empty-array<K, V>` back to `array<K, V>`.
///
/// The empty shape supplies the one value the refinement rules out, so
/// between them the two members are exactly the unrefined base. This is
/// the join a conditional element write produces (`$rows = []; if ($c) {
/// $rows[$id] = …; }`), where spelling both halves out would read as two
/// different types and would let a read of any key pick up the empty
/// half's missing-key `null`.
///
/// Only array refinements pair with an empty *array* shape; a
/// `non-empty-string` beside one is an unrelated union member.
fn absorb_empty_shape_into_non_empty_array(types: &mut Vec<PhpType>) -> bool {
    if !types.iter().any(PhpType::is_empty_array_shape)
        || !types.iter().any(is_non_empty_array_refinement)
    {
        return false;
    }
    for ty in types.iter_mut() {
        if is_non_empty_array_refinement(ty)
            && let Some(base) = unrefined_base(ty)
        {
            *ty = base;
        }
    }
    types.retain(|ty| !ty.is_empty_array_shape());
    true
}

/// Whether a type is a `non-empty-` refinement of an array type, as
/// opposed to `non-empty-string` and friends.
fn is_non_empty_array_refinement(ty: &PhpType) -> bool {
    is_non_empty_refinement(ty) && ty.is_array_like()
}

/// The name a `non-empty-` type refines, or `None` for anything else.
fn non_empty_base_name(name: &str) -> Option<&str> {
    const PREFIX: &str = "non-empty-";
    let (prefix, base) = name.split_at_checked(PREFIX.len())?;
    (!base.is_empty() && prefix.eq_ignore_ascii_case(PREFIX)).then_some(base)
}

fn is_non_empty_refinement(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => non_empty_base_name(name).is_some(),
        TypeKind::Generic(generic) => non_empty_base_name(&generic.name).is_some(),
        _ => false,
    }
}

/// The unrefined form of a `non-empty-` type: `non-empty-array<K, V>`
/// widens to `array<K, V>`, `non-empty-string` to `string`.
fn unrefined_base(ty: &PhpType) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Named(name) => non_empty_base_name(name).map(|base| PhpType::named(atom(base))),
        TypeKind::Generic(generic) => non_empty_base_name(&generic.name)
            .map(|base| PhpType::generic_atom(atom(base), generic.args.clone())),
        _ => None,
    }
}

/// Absorb scalar refinements into their parent types.
///
/// When a union contains both a refinement and its parent (e.g.
/// `positive-int | int`), the refinement is redundant and removed.
pub(crate) fn absorb_scalar_refinements(types: &mut Vec<PhpType>) {
    let mut keep = vec![true; types.len()];

    // Preserve the first member of mutually equivalent aliases (`int` and
    // `integer`, `float` and `double`) instead of letting each absorb the
    // other. Proper subtypes are still removed regardless of order.
    for index in 0..types.len() {
        let TypeKind::Named(subtype) = types[index].kind() else {
            continue;
        };
        for (candidate, candidate_type) in types.iter().enumerate() {
            if index == candidate {
                continue;
            }
            let TypeKind::Named(supertype) = candidate_type.kind() else {
                continue;
            };
            if !is_named_subtype(subtype, supertype) {
                continue;
            }

            // `int` (and its refinements) is a subtype of `float` for
            // compatibility checks (PHP silently widens an int to a
            // float), but the two remain distinct scalar domains rather
            // than one refining the other: `int|float` must stay
            // `int|float`, not collapse to `float`, the way a genuine
            // same-domain refinement (`positive-int|int` → `int`) does.
            if normalize_alias(&supertype.to_ascii_lowercase()) == "float"
                && normalize_alias(&subtype.to_ascii_lowercase()) != "float"
            {
                continue;
            }

            let equivalent = is_named_subtype(supertype, subtype);
            if !equivalent || candidate < index {
                keep[index] = false;
                break;
            }
        }
    }

    let has_kept_name = |names: &[&str]| {
        types.iter().enumerate().any(|(index, ty)| {
            keep[index]
                && matches!(ty.kind(), TypeKind::Named(name)
                    if names.contains(&keyword_lowercase(name).as_str()))
        })
    };
    let has_int = has_kept_name(&["int", "integer"]);
    let has_float = has_kept_name(&["float", "double", "real"]);
    let has_string = has_kept_name(&["string"]);
    for (index, ty) in types.iter().enumerate() {
        let TypeKind::Literal(value) = ty.kind() else {
            continue;
        };
        keep[index] = !match &**value {
            LiteralValue::Int(_) => has_int,
            LiteralValue::Float(_) => has_float,
            LiteralValue::String(_) => has_string,
        };
    }

    crate::util::retain_by_mask(types, &keep);
}
