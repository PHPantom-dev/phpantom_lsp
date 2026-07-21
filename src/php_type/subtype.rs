//! Subtype and equivalence checks.

use super::*;

impl PhpType {
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
pub(crate) fn is_named_subtype(sub: &str, sup: &str) -> bool {
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
pub(crate) fn normalize_alias(name: &str) -> &str {
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
pub(crate) fn literal_is_subtype_of(lit: &LiteralValue, supertype: &PhpType) -> bool {
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

pub(crate) fn literals_equal(left: &LiteralValue, right: &LiteralValue) -> bool {
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
