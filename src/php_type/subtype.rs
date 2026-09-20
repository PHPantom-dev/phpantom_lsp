//! Subtype and equivalence checks.

use super::*;

impl PhpType {
    pub fn equivalent(&self, other: &PhpType) -> bool {
        if self == other {
            return true;
        }
        match (self.kind(), other.kind()) {
            (TypeKind::Named(a), TypeKind::Named(b)) => {
                Self::short_name_of(a) == Self::short_name_of(b)
            }
            (TypeKind::Nullable(a), TypeKind::Nullable(b)) => a.equivalent(b),
            // `?X` is equivalent to `X|null` — normalise Nullable to a
            // two-element Union before comparing so that both notations
            // are treated as identical.
            (TypeKind::Nullable(inner), TypeKind::Union(members))
            | (TypeKind::Union(members), TypeKind::Nullable(inner)) => {
                let as_union = PhpType::union(vec![inner.clone(), PhpType::null()]);
                as_union.equivalent(&PhpType::union(members.to_vec()))
            }
            (TypeKind::Union(a), TypeKind::Union(b))
            | (TypeKind::Intersection(a), TypeKind::Intersection(b)) => {
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
            (TypeKind::Generic(a), TypeKind::Generic(b)) => {
                Self::short_name_of(&a.name) == Self::short_name_of(&b.name)
                    && a.args.len() == b.args.len()
                    && a.args
                        .iter()
                        .zip(b.args.iter())
                        .all(|(x, y)| x.equivalent(y))
            }
            (TypeKind::Array(a), TypeKind::Array(b)) => a.equivalent(b),
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

        if supertype.is_mixed() {
            return true;
        }

        // ── Nullable normalisation ──────────────────────────────────
        // Treat `?T` as `T|null` for uniform handling.
        if let TypeKind::Nullable(inner) = self.kind() {
            let as_union = PhpType::union(vec![inner.clone(), PhpType::null()]);
            return as_union.is_subtype_of(supertype);
        }
        if let TypeKind::Nullable(inner) = supertype.kind() {
            let as_union = PhpType::union(vec![inner.clone(), PhpType::null()]);
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
            let as_union = PhpType::union(vec![PhpType::int(), PhpType::string()]);
            return as_union.is_subtype_of(supertype);
        }

        // ── Union subtype: every member must be a subtype ───────────
        if let TypeKind::Union(members) = self.kind() {
            return members.iter().all(|m| m.is_subtype_of(supertype));
        }

        // ── Union supertype: at least one member must accept self ────
        if let TypeKind::Union(members) = supertype.kind() {
            return members.iter().any(|m| self.is_subtype_of(m));
        }

        // ── Intersection subtype: at least one member suffices ──────
        if let TypeKind::Intersection(members) = self.kind() {
            return members.iter().any(|m| m.is_subtype_of(supertype));
        }

        // ── Intersection supertype: all members required ────────────
        if let TypeKind::Intersection(members) = supertype.kind() {
            return members.iter().all(|m| self.is_subtype_of(m));
        }

        // ── Named ↔ Named scalar subtyping ──────────────────────────
        if let (TypeKind::Named(sub), TypeKind::Named(sup)) = (self.kind(), supertype.kind()) {
            return is_named_subtype(sub, sup);
        }

        // ── StaticType / ThisType <: bound class ────────────────────
        // StaticType(A) <: A and ThisType(A) <: A always hold.
        // ThisType(A) <: StaticType(A) also holds ($this is more specific).
        if let TypeKind::StaticType(sub) | TypeKind::ThisType(sub) = self.kind() {
            match supertype.kind() {
                TypeKind::Named(sup) | TypeKind::StaticType(sup) => {
                    return is_named_subtype(sub, sup);
                }
                _ => {}
            }
        }

        // ── Literal subtyping ───────────────────────────────────────
        if let TypeKind::Literal(lit) = self.kind() {
            return literal_is_subtype_of(lit, supertype);
        }

        // ── IntRange <: int / refined-int / IntRange ────────────────
        if let TypeKind::IntRange(sub_min, sub_max) = self.kind() {
            match supertype.kind() {
                TypeKind::Named(sup) => {
                    let sup_l = sup.to_ascii_lowercase();
                    // `float` and `number` are on this list for the same
                    // reason a bare `int` is a subtype of them: PHP widens
                    // an integer to a float on the way in, in strict mode
                    // too.  A range is still an integer, so bounding it
                    // takes nothing away from that.
                    if matches!(
                        sup_l.as_str(),
                        "int"
                            | "integer"
                            | "float"
                            | "double"
                            | "real"
                            | "numeric"
                            | "number"
                            | "scalar"
                            | "array-key"
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
                TypeKind::IntRange(sup_min, sup_max) => {
                    return int_range_is_subrange(sub_min, sub_max, sup_min, sup_max);
                }
                _ => {}
            }
        }

        // ── refined-int <: IntRange ─────────────────────────────────
        // e.g. non-negative-int <: int<0,max>, positive-int <: int<0,max>
        if let TypeKind::Named(sub) = self.kind()
            && let TypeKind::IntRange(sup_min, sup_max) = supertype.kind()
        {
            let sub_l = sub.to_ascii_lowercase();
            if let Some((sub_min, sub_max)) = refined_int_to_range(&sub_l) {
                return int_range_is_subrange(sub_min, sub_max, sup_min, sup_max);
            }
        }

        // ── Array slice: T[] <: array ───────────────────────────────
        if let TypeKind::Array(inner_sub) = self.kind() {
            match supertype.kind() {
                TypeKind::Named(sup) => {
                    return matches!(sup.to_ascii_lowercase().as_str(), "array" | "iterable");
                }
                TypeKind::Array(inner_sup) => {
                    return inner_sub.is_subtype_of(inner_sup);
                }
                TypeKind::Generic(g) if is_array_like_name(&g.name) => {
                    // T[] <: array<int, T2> when T <: T2
                    if let Some(val) = g.args.last() {
                        return inner_sub.is_subtype_of(val);
                    }
                }
                _ => {}
            }
        }

        // ── ArrayShape <: array / iterable ──────────────────────────
        if let TypeKind::ArrayShape(entries) = self.kind() {
            // A shape satisfies a `non-empty-…` supertype only when it
            // names a key that is always there. `array{}` never does, and
            // neither does a shape whose every entry is optional.
            let is_non_empty = entries.iter().any(|entry| !entry.optional);
            // `list{…}` says so outright; a shape tracked from a literal or
            // from appends says so by holding `0, 1, 2, …` in order.
            let is_list = self.is_list_shape() || shape_keys_are_sequential(entries);

            if let TypeKind::Named(sup) = supertype.kind() {
                return match sup.to_ascii_lowercase().as_str() {
                    "array" | "iterable" => true,
                    "non-empty-array" => is_non_empty,
                    "list" => is_list,
                    "non-empty-list" => is_list && is_non_empty,
                    _ => false,
                };
            }

            // ArrayShape <: array<K, V>  (or other generic array-like)
            // Every shape key must be a subtype of K, every value a subtype of V.
            if let TypeKind::Generic(g) = supertype.kind()
                && is_array_like_name(&g.name)
            {
                if is_non_empty_array_name(&g.name) && !is_non_empty {
                    return false;
                }
                if is_list_name(&g.name) && !is_list {
                    return false;
                }
                match g.args.len() {
                    // array<V> — only check values.
                    1 => {
                        let val_type = &g.args[0];
                        return entries.iter().all(|e| e.value_type.is_subtype_of(val_type));
                    }
                    // array<K, V> — check both keys and values.
                    2 => {
                        let key_type = &g.args[0];
                        let val_type = &g.args[1];
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
            if let TypeKind::Array(inner) = supertype.kind() {
                return entries.iter().all(|e| e.value_type.is_subtype_of(inner));
            }
        }

        // ── ObjectShape <: object ───────────────────────────────────
        if matches!(self.kind(), TypeKind::ObjectShape(_))
            && let TypeKind::Named(sup) = supertype.kind()
        {
            return sup.eq_ignore_ascii_case("object");
        }

        // ── Generic covariance (array-like containers) ──────────────
        if let (TypeKind::Generic(sub), TypeKind::Generic(sup)) = (self.kind(), supertype.kind()) {
            let base_sub = sub.name.to_ascii_lowercase();
            let base_sup = sup.name.to_ascii_lowercase();

            // Same base or compatible bases (list <: array, etc.)
            let bases_compatible = base_sub == base_sup
                || (is_array_like_name(&sub.name) && is_array_like_name(&sup.name));

            if bases_compatible && sub.args.len() == sup.args.len() {
                return sub
                    .args
                    .iter()
                    .zip(sup.args.iter())
                    .all(|(s, t)| s.is_subtype_of(t));
            }

            // Array-like containers spell the same type at different
            // arities: `list<V>` is `array<int, V>`, `array<V>` is
            // `array<array-key, V>`. Compare the implied key/value pair so
            // that a `list<T>` still satisfies a declared `array<int, T>`.
            if is_array_like_name(&sub.name)
                && is_array_like_name(&sup.name)
                && let Some((sub_key, sub_val)) = array_like_key_value(sub)
                && let Some((sup_key, sup_val)) = array_like_key_value(sup)
            {
                // A `list` supertype demands sequential integer keys,
                // which a plain `array` cannot promise.
                if is_list_name(&sup.name) && !is_list_name(&sub.name) {
                    return false;
                }
                return sub_key.is_subtype_of(&sup_key) && sub_val.is_subtype_of(sup_val);
            }
        }

        // Generic array-like <: bare `array` / `iterable`
        if let TypeKind::Generic(g) = self.kind()
            && is_array_like_name(&g.name)
            && let TypeKind::Named(sup) = supertype.kind()
        {
            return matches!(sup.to_ascii_lowercase().as_str(), "array" | "iterable");
        }

        // ── class-string / interface-string subtyping ───────────────
        //
        // Both name a PHP symbol, so every string refinement the named
        // lattice already records for the spelling holds of them too:
        // they are non-empty, never `"0"`, and usable as an array key.
        // Reaching that table through the kind's own name is what keeps
        // the two ways of spelling the type — a dedicated `ClassString`
        // node and a bare `Named("class-string")` — answering alike,
        // rather than the dedicated node knowing only `string`.
        match (self.kind(), supertype.kind()) {
            (TypeKind::ClassString(_), TypeKind::Named(sup)) => {
                return is_named_subtype("class-string", sup);
            }
            (TypeKind::ClassString(Some(sub_inner)), TypeKind::ClassString(Some(sup_inner))) => {
                return sub_inner.is_subtype_of(sup_inner);
            }
            (TypeKind::ClassString(Some(_)), TypeKind::ClassString(None)) => {
                return true;
            }
            (TypeKind::InterfaceString(_), TypeKind::Named(sup)) => {
                return is_named_subtype("interface-string", sup);
            }
            _ => {}
        }

        // ── Callable subtyping ──────────────────────────────────────
        if let (TypeKind::Callable(sub), TypeKind::Callable(sup)) = (self.kind(), supertype.kind())
        {
            let (params_sub, ret_sub) = (&sub.params, &sub.return_type);
            let (params_sup, ret_sup) = (&sup.params, &sup.return_type);
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
        if matches!(self.kind(), TypeKind::Callable(_))
            && let TypeKind::Named(sup) = supertype.kind()
        {
            return matches!(
                sup.to_ascii_lowercase().as_str(),
                "callable" | "closure" | "object"
            );
        }

        // Bare `Closure` or `callable` <: callable specification.
        // A bare `Closure` might have any signature — we cannot prove
        // it violates the specification, so treat it as compatible.
        if let TypeKind::Named(sub) = self.kind()
            && matches!(sub.to_ascii_lowercase().as_str(), "callable" | "closure")
            && matches!(supertype.kind(), TypeKind::Callable(_))
        {
            return true;
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Array-like arity normalisation
// ---------------------------------------------------------------------------

/// The `(key, value)` pair an array-like generic implies, whichever arity
/// it was written at.
///
/// `list<V>` and `non-empty-list<V>` key on `int`, `array<V>` on
/// `array-key`, and a one-argument `iterable<V>` on `mixed` (a
/// `Traversable` may yield any key type). Returns `None` for arities that
/// carry no key/value meaning.
pub(crate) fn array_like_key_value(generic: &GenericType) -> Option<(PhpType, &PhpType)> {
    match generic.args.as_slice() {
        [value] => {
            let name = generic.name.to_ascii_lowercase();
            let key = match name.as_str() {
                "list" | "non-empty-list" => PhpType::int(),
                "iterable" => PhpType::mixed(),
                // `array-key`, spelled as the union the checks above
                // normalise it to anyway.
                _ => PhpType::union(vec![PhpType::int(), PhpType::string()]),
            };
            Some((key, value))
        }
        [key, value] => Some((key.clone(), value)),
        _ => None,
    }
}

/// Whether a shape's entries occupy the keys `0, 1, 2, …` a `list`
/// promises.
///
/// Positional entries take the next index, an explicit key must spell that
/// same index out, and a string key rules the shape out entirely. Optional
/// entries are only allowed at the end: an absent one in the middle would
/// leave a hole in the sequence.
fn shape_keys_are_sequential(entries: &[ShapeEntry]) -> bool {
    let mut seen_optional = false;
    for (position, entry) in (0_i64..).zip(entries) {
        if seen_optional && !entry.optional {
            return false;
        }
        seen_optional |= entry.optional;
        match entry.key.as_deref() {
            None => {}
            Some(key) if key.parse::<i64>() == Ok(position) => {}
            Some(_) => return false,
        }
    }
    true
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
pub(crate) fn is_self_ref_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("self")
        || name.eq_ignore_ascii_case("static")
        || name.eq_ignore_ascii_case("$this")
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
/// - `list <: array`, `non-empty-list <: array`, `non-empty-array <: array`,
///   `associative-array <: array`
/// - `callable <: object` is NOT true (callables can be strings/arrays)
pub(crate) fn is_named_subtype(sub: &str, sup: &str) -> bool {
    let sub_raw = sub.strip_prefix('\\').unwrap_or(sub);
    let sup_raw = sup.strip_prefix('\\').unwrap_or(sup);
    let sub_l = sub_raw.to_ascii_lowercase();
    let sup_l = sup_raw.to_ascii_lowercase();

    // `number`, `real`, `integer`, `boolean`, `double`, and `resource` are
    // pseudo-types/aliases only in their exact lowercase spelling (see
    // `is_lowercase_only_pseudo_type`). A differently-cased bare `Number`
    // (e.g. `BcMath\Number`) or `Integer` is a real class, so it must not
    // reach case-insensitive equality or alias normalisation below; nominal
    // class relationships are resolved by the caller's hierarchy check.
    let sub_is_pseudo_class = is_lowercase_only_pseudo_type(&sub_l) && sub_raw != sub_l;
    let sup_is_pseudo_class = is_lowercase_only_pseudo_type(&sup_l) && sup_raw != sup_l;

    if sub_l == sup_l {
        // Two spellings of `Real` are the same class; `Real` and `real` are not
        // the same thing.
        return sub_is_pseudo_class == sup_is_pseudo_class;
    }

    if sub_is_pseudo_class || sup_is_pseudo_class {
        return false;
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

        // An interface and an enum are class-likes, so the string that
        // names one is a `class-string`.  A trait is not — `trait-string`
        // is its own thing, and PHPStan keeps the two apart.
        "class-string" => matches!(sub_n, "interface-string" | "enum-string"),

        // `non-falsy-string` and its Psalm synonym `truthy-string` exclude
        // both `""` and `"0"`, so they are strictly narrower than
        // `non-empty-string`, which excludes only `""`.
        "non-empty-string" => matches!(
            sub_n,
            "non-empty-literal-string"
                | "non-empty-lowercase-string"
                | "non-empty-uppercase-string"
                | "callable-string"
                | "class-string"
                | "interface-string"
                | "trait-string"
                | "enum-string"
                | "truthy-string"
                | "non-falsy-string"
        ),

        // The `non-empty-*` refinements are absent here on purpose: each
        // one still admits `"0"`, which is falsy, so none of them is a
        // subtype of the strict arm. What remains are the string kinds
        // that name a PHP symbol, and a symbol name can never be `"0"`.
        "truthy-string" | "non-falsy-string" => matches!(
            sub_n,
            "callable-string"
                | "class-string"
                | "interface-string"
                | "trait-string"
                | "enum-string"
                | "truthy-string"
                | "non-falsy-string"
        ),

        "literal-string" => matches!(sub_n, "non-empty-literal-string"),

        "lowercase-string" => matches!(sub_n, "non-empty-lowercase-string"),

        "uppercase-string" => matches!(sub_n, "non-empty-uppercase-string"),

        // ── numeric supertypes ──────────────────────────────────
        "numeric" => matches!(
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
        "number" => matches!(
            sub_n,
            "int"
                | "float"
                | "positive-int"
                | "negative-int"
                | "non-positive-int"
                | "non-negative-int"
                | "non-zero-int"
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
pub(crate) fn normalize_alias(name: &str) -> &str {
    match name {
        "integer" => "int",
        "double" | "real" => "float",
        "boolean" => "bool",
        "no-return" | "noreturn" | "never-return" | "never-returns" => "never",
        "non-empty-mixed" => "mixed",
        other => other,
    }
}

/// Check whether a literal type is a subtype of a given supertype.
pub(crate) fn literal_is_subtype_of(lit: &LiteralValue, supertype: &PhpType) -> bool {
    match supertype.kind() {
        TypeKind::Literal(other_lit) => literals_equal(lit, other_lit),
        TypeKind::IntRange(min, max) => lit
            .parse_i64()
            .is_some_and(|value| int_literal_is_within_range(value, min, max)),
        TypeKind::Named(sup) => {
            // A differently-cased bare `Number` (e.g. `BcMath\Number`) or
            // `Real` is a real class, not the lowercase pseudo-type; a scalar
            // literal is never a subtype of it. `keyword_lowercase` keeps such
            // a spelling as-is, so it matches none of the arms below.
            let sup_l = keyword_lowercase(sup);
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
                    "float" | "double" | "real" | "numeric" | "number" | "scalar"
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

                // A numeric string is also part of the broader `numeric`
                // value domain. It is not part of `number`, which represents
                // only int|float runtime values.
                if matches!(sup_l.as_str(), "numeric-string" | "numeric") && lit.is_numeric_string()
                {
                    return true;
                }

                // Lowercase/uppercase string refinements are exactly what
                // `strtolower`/`strtoupper` would leave unchanged; a content
                // with no cased characters at all (digits, punctuation, "")
                // satisfies both.
                if matches!(
                    sup_l.as_str(),
                    "lowercase-string" | "non-empty-lowercase-string"
                ) && !content.bytes().any(|b| b.is_ascii_uppercase())
                    && (sup_l != "non-empty-lowercase-string" || !content.is_empty())
                {
                    return true;
                }
                if matches!(
                    sup_l.as_str(),
                    "uppercase-string" | "non-empty-uppercase-string"
                ) && !content.bytes().any(|b| b.is_ascii_lowercase())
                    && (sup_l != "non-empty-uppercase-string" || !content.is_empty())
                {
                    return true;
                }

                // `callable-string` enforcement needs the function/method
                // symbol table, which this layer cannot see; stay silent
                // rather than reject every function-name literal.
                if sup_l == "callable-string" {
                    return true;
                }

                return false;
            }
            false
        }
        _ => false,
    }
}

pub(crate) fn literals_equal(left: &LiteralValue, right: &LiteralValue) -> bool {
    if left == right {
        return true;
    }

    match (left, right) {
        (LiteralValue::Int(left_raw), LiteralValue::Int(right_raw)) => {
            match (left.parse_i64(), right.parse_i64()) {
                (Some(left), Some(right)) => left == right,
                _ => left_raw == right_raw,
            }
        }
        (LiteralValue::Float(left_raw), LiteralValue::Float(right_raw)) => {
            match (left.parse_f64(), right.parse_f64()) {
                (Some(left), Some(right)) => left == right,
                _ => left_raw == right_raw,
            }
        }
        (LiteralValue::String(left_raw), LiteralValue::String(right_raw)) => {
            match (left.string_content(), right.string_content()) {
                (Some(left), Some(right)) => left == right,
                _ => left_raw == right_raw,
            }
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
pub(crate) fn refined_int_to_range(name: &str) -> Option<(&'static str, &'static str)> {
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
pub(crate) fn parse_range_bound(s: &str) -> i64 {
    match s.trim().to_ascii_lowercase().as_str() {
        "min" => i64::MIN,
        "max" => i64::MAX,
        v => v.parse::<i64>().unwrap_or(0),
    }
}

/// Check whether range `(sub_min, sub_max)` is fully contained within
/// `(sup_min, sup_max)`.  Both bounds are inclusive.
pub(crate) fn int_range_is_subrange(
    sub_min: &str,
    sub_max: &str,
    sup_min: &str,
    sup_max: &str,
) -> bool {
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
