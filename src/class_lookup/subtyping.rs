//! Subtype checks between classes and between types: the nominal walk up
//! a class's parents and interfaces, and the structural rules for unions,
//! generics, shapes and callables built on it.

use crate::atom::atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::TypeKind;
use crate::types::{ClassInfo, ClassLikeKind};

/// Check whether `child` is a subclass (direct or transitive) of
/// `parent`, including implemented interfaces.
///
/// Returns `false` if `child` cannot be loaded or if there is no
/// inheritance relationship.  Delegates to the shared nominal subtype
/// walk ([`is_subtype_of`]), which handles
/// transitive interface extension, FQN normalisation, and cycle
/// detection.
pub(crate) fn is_subclass_of(
    child: &str,
    parent: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    if child.eq_ignore_ascii_case(parent) {
        return false; // same class, not a subclass
    }
    match class_loader(Fqn::new(child)) {
        Some(child_class) => is_subtype_of(&child_class, parent, class_loader),
        None => false,
    }
}

/// Check whether `class` is a subtype of the class identified by
/// `ancestor_name`.  Returns `true` when:
///
/// - `class.name` equals `ancestor_name` (same class), or
/// - walking the `parent_class` chain reaches `ancestor_name`, or
/// - `ancestor_name` appears in the `interfaces` list of `class` or any
///   of its ancestors.
///
/// Both short names and fully-qualified names are compared so that
/// cross-file relationships (where `parent_class` stores FQNs) work.
pub(crate) fn is_subtype_of(
    class: &ClassInfo,
    ancestor_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    // Every name handled by this function — the ancestor plus each
    // `interfaces`/`parent_class` entry pulled from a loaded class — is a
    // canonical FQN, not a user-typed reference.  A backslash-free name
    // (e.g. `Iterator`, `Traversable`, `Exception`) is therefore a
    // root-namespace class.
    let load_fqn = |name: &str| class_loader(Fqn::new(name));

    // Resolve the ancestor to its FQN so that all comparisons below are
    // FQN-vs-FQN.  When `ancestor_name` is already a FQN (contains `\`)
    // we use it directly.  When it is a short name we try to load it
    // through `load_fqn` and use the loaded class's FQN.  For
    // root-namespace classes (e.g. `RuntimeException`) the FQN equals the
    // short name, so the fallback to `ancestor_name` is correct.
    let ancestor_fqn: String = if ancestor_name.contains('\\') {
        ancestor_name.to_string()
    } else if let Some(loaded) = load_fqn(ancestor_name) {
        loaded.fqn().to_string()
    } else {
        // Cannot resolve — keep the original name.  For root-namespace
        // classes this is already the FQN.
        ancestor_name.to_string()
    };
    let ancestor = ancestor_fqn.as_str();

    // Same class?  Always compare by FQN.
    if class.fqn() == ancestor {
        return true;
    }

    // Check interfaces on the class itself (stored as FQNs after
    // resolve_parent_class_names), walking the full interface
    // inheritance tree so that transitive relationships are found
    // (e.g. Response implements ResponseInterface extends MessageInterface).
    let mut iface_queue: Vec<String> = class.interfaces.iter().map(|a| a.to_string()).collect();
    let mut visited_ifaces: std::collections::HashSet<String> =
        iface_queue.iter().cloned().collect();
    while let Some(iface_name) = iface_queue.pop() {
        if iface_name == ancestor {
            return true;
        }
        // Load the interface and check its parents (interface extends).
        if let Some(iface_info) = load_fqn(&iface_name) {
            // Interface parents are stored in both `parent_class`
            // (first parent for single-extends compat) and
            // `interfaces` (all parents for multi-extends).
            for parent_iface in &iface_info.interfaces {
                if visited_ifaces.insert(parent_iface.to_string()) {
                    iface_queue.push(parent_iface.to_string());
                }
            }
            if let Some(ref pc) = iface_info.parent_class
                && visited_ifaces.insert(pc.to_string())
            {
                iface_queue.push(pc.to_string());
            }
        }
    }

    // Walk the parent class chain (parent_class is also a resolved FQN).
    let mut current_parent = class.parent_class.map(|a| a.to_string());
    let mut visited_parents: std::collections::HashSet<String> = std::collections::HashSet::new();
    visited_parents.insert(class.fqn().to_string());
    let mut depth = 0u32;
    while let Some(ref name) = current_parent {
        depth += 1;
        if depth > 20 {
            break;
        }
        if name == ancestor {
            return true;
        }
        // Load the parent to check its interfaces (transitively)
        // and continue the class chain.  `load_fqn` resolves
        // root-namespace names globally, so the use-map can no longer
        // shadow a global parent (e.g. a same-file `use App\Exception;`
        // does not hijack a stub class's global `\Exception` parent).
        if let Some(parent_info) = load_fqn(name) {
            let mut p_iface_queue: Vec<String> = parent_info
                .interfaces
                .iter()
                .map(|a| a.to_string())
                .collect();
            let mut p_visited: std::collections::HashSet<String> =
                p_iface_queue.iter().cloned().collect();
            while let Some(iface_name) = p_iface_queue.pop() {
                if iface_name == ancestor {
                    return true;
                }
                if let Some(iface_info) = load_fqn(&iface_name) {
                    for pi in &iface_info.interfaces {
                        if p_visited.insert(pi.to_string()) {
                            p_iface_queue.push(pi.to_string());
                        }
                    }
                    if let Some(ref pc) = iface_info.parent_class
                        && p_visited.insert(pc.to_string())
                    {
                        p_iface_queue.push(pc.to_string());
                    }
                }
            }
            // Cycle detection: stop if we have already visited this
            // parent's FQN (a malformed or self-referential hierarchy).
            if !visited_parents.insert(parent_info.fqn().to_string()) {
                break;
            }
            current_parent = parent_info.parent_class.map(|a| a.to_string());
        } else {
            break;
        }
    }

    // The parser records the implicit `Stringable` on every class that
    // declares `__toString()` itself, but PHP also adds it when the
    // method comes from a trait, which is only known once traits load.
    if ancestor.eq_ignore_ascii_case("Stringable") && class.kind != ClassLikeKind::Interface {
        let declares = |c: &ClassInfo| {
            c.methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case("__toString"))
        };
        return crate::inheritance::find_declaring_trait(
            &class.used_traits,
            class_loader,
            &declares,
        )
        .is_some()
            || crate::inheritance::ancestors(class, class_loader).any(|(_, parent)| {
                crate::inheritance::find_declaring_trait(
                    &parent.used_traits,
                    class_loader,
                    &declares,
                )
                .is_some()
            });
    }

    false
}

/// Convenience wrapper around [`is_subtype_of_typed`] that accepts bare
/// class names instead of pre-constructed [`crate::php_type::PhpType`] values.
///
/// This avoids the boilerplate of wrapping each name in
/// `PhpType::named(atom(name.as_ref()))` at call sites that already have
/// `&str` class names.
pub(crate) fn is_subtype_of_names(
    subtype_name: &str,
    supertype_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    use crate::php_type::PhpType;
    is_subtype_of_typed(
        &PhpType::named(atom(subtype_name)),
        &PhpType::named(atom(supertype_name)),
        class_loader,
    )
}

/// Like [`is_subtype_of_typed`] but accepts a `&str` for the supertype,
/// avoiding `TypeKind::Named` wrapping at call sites that already have a
/// `&PhpType` subtype and a bare class name as supertype.
pub(crate) fn is_subtype_of_named(
    subtype: &crate::php_type::PhpType,
    supertype_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    use crate::php_type::PhpType;
    is_subtype_of_typed(subtype, &PhpType::named(atom(supertype_name)), class_loader)
}

/// Check whether `subtype` is a subtype of `supertype`, combining
/// structural subtyping ([`crate::php_type::PhpType::is_subtype_of`])
/// with nominal class-hierarchy walking ([`is_subtype_of`]).
///
/// This is the single entry point for all subtype checks that need
/// both layers:
///
/// - Scalars, unions, intersections, generics, callables, literals,
///   and other structural relationships are handled by
///   `PhpType::is_subtype_of`.
/// - Nominal class relationships (`Cat <: Animal`) are resolved by
///   loading the class via `class_loader` and walking its parent
///   chain and interface list.
///
/// Returns `true` when the structural check succeeds, or when both
/// types are named (class/interface) types and the class hierarchy
/// confirms the relationship.
pub(crate) fn is_subtype_of_typed(
    subtype: &crate::php_type::PhpType,
    supertype: &crate::php_type::PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    use crate::php_type::PhpType;

    // Fast path: structural subtyping covers scalars, unions,
    // intersections, generics, callables, literals, etc.
    if subtype.is_subtype_of(supertype) {
        return true;
    }

    if crate::virtual_members::laravel::has_model_type_operator(subtype)
        || crate::virtual_members::laravel::has_model_type_operator(supertype)
    {
        let sub = crate::virtual_members::laravel::expand_model_type(subtype, class_loader);
        let sup = crate::virtual_members::laravel::expand_model_type(supertype, class_loader);
        if &sub != subtype || &sup != supertype {
            return is_subtype_of_typed(&sub, &sup, class_loader);
        }
    }

    // ── Union subtype: every member must be a subtype ───────────
    if let TypeKind::Union(members) = subtype.kind() {
        return members
            .iter()
            .all(|m| is_subtype_of_typed(m, supertype, class_loader));
    }

    // ── Union supertype: at least one member must accept subtype ─
    if let TypeKind::Union(members) = supertype.kind() {
        return members
            .iter()
            .any(|m| is_subtype_of_typed(subtype, m, class_loader));
    }

    // ── Nullable normalisation ──────────────────────────────────
    if let TypeKind::Nullable(inner) = subtype.kind() {
        let as_union = PhpType::union(vec![inner.clone(), PhpType::null()]);
        return is_subtype_of_typed(&as_union, supertype, class_loader);
    }
    if let TypeKind::Nullable(inner) = supertype.kind() {
        let as_union = PhpType::union(vec![inner.clone(), PhpType::null()]);
        return is_subtype_of_typed(subtype, &as_union, class_loader);
    }

    // ── Intersection supertype: all members required ────────────
    // Checked before the subtype-intersection branch so that the
    // both-intersections case (e.g. `A&B&C` <: `A&B`) decomposes the
    // supertype first: every supertype member must be satisfied by
    // *some* subtype member.  Handling the subtype first would instead
    // demand that a single member satisfy the whole supertype, wrongly
    // rejecting a narrower intersection that carries extra constraints.
    if let TypeKind::Intersection(members) = supertype.kind() {
        return members
            .iter()
            .all(|m| is_subtype_of_typed(subtype, m, class_loader));
    }

    // ── Intersection subtype: at least one member suffices ──────
    if let TypeKind::Intersection(members) = subtype.kind() {
        return members
            .iter()
            .any(|m| is_subtype_of_typed(m, supertype, class_loader));
    }

    // ── Class-constant shape keys ───────────────────────────────
    // A docblock keeps a key spelled `Slots::NAME` as written, which the
    // structural check can only read as some `array-key`. Evaluated, it is
    // the integer or string the constant holds, which may well be the `0`
    // a list needs.
    let sub_evaluated = shape_with_constant_keys_evaluated(subtype, class_loader);
    let sup_evaluated = shape_with_constant_keys_evaluated(supertype, class_loader);
    if sub_evaluated.is_some() || sup_evaluated.is_some() {
        return is_subtype_of_typed(
            sub_evaluated.as_ref().unwrap_or(subtype),
            sup_evaluated.as_ref().unwrap_or(supertype),
            class_loader,
        );
    }

    // ── Shape <: shape ──────────────────────────────────────────
    // The structural check compared the entries each shape lists and found
    // them wanting, but it takes the class names in them at face value, so
    // the comparison is redone with the hierarchy applied to the entries.
    //
    // It also has to be answered here, ahead of the generic-array rules
    // below: they read an array by its `kind()`, which for an unsealed shape
    // is the generic array it widens to, and every `array{…, ...}` widens to
    // the same `non-empty-array<array-key, mixed>`. Those rules would call
    // any two of them subtypes, whatever entries they list.
    if let (Some(sub), Some(wider)) = (subtype.shape_parts(), supertype.shape_parts()) {
        return crate::php_type::shape_is_subshape(&sub, &wider, &|a, b| {
            is_subtype_of_typed(a, b, class_loader)
        });
    }

    // ── Array <: unsealed shape ─────────────────────────────────
    // Ahead of the generic-array rules for the same reason: an array that
    // is not itself a shape fits the generic array an unsealed shape widens
    // to, whatever entries the shape requires.
    if let Some(fits) = crate::php_type::array_is_unsealed_subshape(subtype, supertype, &|a, b| {
        is_subtype_of_typed(a, b, class_loader)
    }) {
        return fits;
    }

    // ── Generic covariance with class-loader awareness ──────────
    // The structural `is_subtype_of` compares generic type params
    // by structural equality, which fails when one side uses a
    // namespace-qualified name and the other uses a short name
    // (e.g. `list<Pen>` vs `list<Demo\Pen>`).  Re-check with the
    // class loader so nominal hierarchy applies to inner params.
    if let (TypeKind::Generic(sub), TypeKind::Generic(sup)) = (subtype.kind(), supertype.kind()) {
        let (name_sub, args_sub) = (&sub.name, &sub.args);
        let (name_sup, args_sup) = (&sup.name, &sup.args);
        let base_sub = name_sub.to_ascii_lowercase();
        let base_sup = name_sup.to_ascii_lowercase();
        let bases_compatible = base_sub == base_sup
            || (crate::php_type::is_array_like_name(name_sub)
                && crate::php_type::is_array_like_name(name_sup));
        if bases_compatible && args_sub.len() == args_sup.len() {
            let is_array_like = crate::php_type::is_array_like_name(name_sub)
                || crate::php_type::is_array_like_name(name_sup);
            let all_params_ok = args_sub.iter().zip(args_sup.iter()).all(|(s, t)| {
                if is_array_like {
                    // Arrays are covariant in PHP (read-only semantics)
                    is_subtype_of_typed(s, t, class_loader)
                } else {
                    // Non-array generics are invariant by default
                    // (both directions must hold, or they must be equal)
                    s == t
                        || (is_subtype_of_typed(s, t, class_loader)
                            && is_subtype_of_typed(t, s, class_loader))
                }
            });
            if all_params_ok {
                return true;
            }
            // Type arguments that provably do not line up in *either*
            // direction settle the question here.  The nominal fallback at
            // the end of this function compares base names only, so it
            // would read `Box<string>` <: `Box<int>` as `Box` <: `Box` and
            // discard the arguments entirely.
            //
            // A pair where one direction does hold is still left to it:
            // variance is not recorded on the resolved type, so a wider
            // argument may be a `@template-contravariant` one, or simply
            // all we managed to infer.
            if !is_array_like
                && args_sub.iter().zip(args_sup.iter()).any(|(s, t)| {
                    !is_subtype_of_typed(s, t, class_loader)
                        && !is_subtype_of_typed(t, s, class_loader)
                })
            {
                return false;
            }
        }

        // ── Array-like generics written at different arities ────
        // The same array spells out at one, two, or no type arguments:
        // `list<V>` is `array<int, V>` and `array<V>` is
        // `array<array-key, V>`.  `is_subtype_of` normalises the pair
        // already, but compares the implied key and value structurally,
        // so a nominal inner type (`array<int, Cat>` <: `array<Animal>`)
        // never lines up.  Redo the comparison through the class loader.
        //
        // A `list` supertype is the one form that demands more than its
        // values: an `array` cannot promise the sequential integer keys,
        // so it is left to the checks below.
        if args_sub.len() != args_sup.len()
            && crate::php_type::is_array_like_name(name_sub)
            && crate::php_type::is_array_like_name(name_sup)
            && !(crate::php_type::is_list_name(name_sup)
                && !crate::php_type::is_list_name(name_sub))
            && let Some((sub_key, sub_val)) = crate::php_type::array_like_key_value(sub)
            && let Some((sup_key, sup_val)) = crate::php_type::array_like_key_value(sup)
            && is_subtype_of_typed(&sub_key, &sup_key, class_loader)
            && is_subtype_of_typed(sub_val, sup_val, class_loader)
        {
            return true;
        }
    }

    // ── Array-like generic <: `X[]` ─────────────────────────────
    // `X[]` is `array<mixed, X>`: it constrains the values and says
    // nothing about the keys, so any array-like generic whose value type
    // fits satisfies it — `array<int, Cat>` and `list<Cat>` are both
    // `Animal[]`.  `is_subtype_of` has no rule for this direction at all.
    if let TypeKind::Generic(sub) = subtype.kind()
        && crate::php_type::is_array_like_name(&sub.name)
        && let TypeKind::Array(inner_sup) = supertype.kind()
        && let Some(sub_val) = sub.args.last()
        && is_subtype_of_typed(sub_val, inner_sup, class_loader)
    {
        return true;
    }

    // ── Array slice covariance ──────────────────────────────────
    // The structural `is_subtype_of` compares `X[]` vs `Y[]` by
    // structural equality on the inner type, which misses nominal
    // subclass relationships (e.g. `Cat[]` <: `Animal[]` where
    // `Cat extends Animal`).  Re-check with the class loader so
    // the hierarchy walk applies to inner types.
    if let (TypeKind::Array(inner_sub), TypeKind::Array(inner_sup)) =
        (subtype.kind(), supertype.kind())
        && is_subtype_of_typed(inner_sub, inner_sup, class_loader)
    {
        return true;
    }

    // ── Callable specification <: Closure / object ──────────────
    // A `Closure(int): string` is a Closure instance, which is an
    // object.  The structural check only handles `callable` as
    // the named supertype; extend to `Closure` and `object`.
    if matches!(subtype.kind(), TypeKind::Callable { .. })
        && let Some(sup) = supertype.base_name()
        && (sup.eq_ignore_ascii_case("Closure") || sup.eq_ignore_ascii_case("object"))
    {
        return true;
    }

    // ── class-string covariance through nominal hierarchy ────────
    // The structural `is_subtype_of` handles `class-string<Cat> <:
    // class-string<Animal>` only when `Cat` and `Animal` are
    // structurally equal.  Extend to nominal hierarchy so that
    // `class-string<Cat>` is accepted where `class-string<Animal>`
    // is expected when `Cat extends Animal`.  A bare `class-string`
    // is treated as `class-string<object>`, so it satisfies
    // `class-string<object>` (and its `mixed` equivalent) — any class
    // name is a class-string of some object.
    if let (TypeKind::ClassString(sub_inner), TypeKind::ClassString(sup_inner)) =
        (subtype.kind(), supertype.kind())
    {
        let object_bound = PhpType::named(atom("object"));
        let sub = sub_inner.as_ref().unwrap_or(&object_bound);
        let sup = sup_inner.as_ref().unwrap_or(&object_bound);
        return is_subtype_of_typed(sub, sup, class_loader);
    }

    // ── class-string <: interface-string ────────────────────────
    // `interface-string` constrains the *name* a string holds, not the
    // type that name denotes: `SomeClass::class` is not one even when
    // `SomeClass implements SomeInterface`, while `SomeInterface::class`
    // is.  Only a name we can load and see is not an interface is
    // rejected — a bare `class-string`, and one whose class we cannot
    // load, stay silent.
    if let (TypeKind::ClassString(sub_inner), TypeKind::InterfaceString(sup_inner)) =
        (subtype.kind(), supertype.kind())
    {
        if let (Some(sub), Some(sup)) = (sub_inner, sup_inner)
            && !is_subtype_of_typed(sub, sup, class_loader)
        {
            return false;
        }
        return sub_inner
            .as_ref()
            .and_then(|inner| inner.base_name())
            .and_then(|name| class_loader(Fqn::new(name)))
            .is_none_or(|cls| cls.kind == ClassLikeKind::Interface);
    }

    // ── interface-string <: class-string / interface-string ─────
    // The value names a class-like either way, so only the bounds are
    // left to line up.  A missing bound is `object`, which anything
    // satisfies.
    if let TypeKind::InterfaceString(sub_inner) = subtype.kind()
        && let TypeKind::ClassString(sup_inner) | TypeKind::InterfaceString(sup_inner) =
            supertype.kind()
    {
        let object_bound = PhpType::named(atom("object"));
        let sub = sub_inner.as_ref().unwrap_or(&object_bound);
        let sup = sup_inner.as_ref().unwrap_or(&object_bound);
        return is_subtype_of_typed(sub, sup, class_loader);
    }

    // ── class-string <: enum-string / trait-string ──────────────
    // The same name-kind rule as `interface-string`: `SomeEnum::class`
    // is an `enum-string`, `SomeClass::class` is not.  A bare
    // `class-string`, and one whose class we cannot load, stay silent.
    if let TypeKind::ClassString(sub_inner) = subtype.kind()
        && let Some((kind, sup_bound)) = class_kind_string(supertype)
    {
        if let (Some(sub), Some(sup)) = (sub_inner, sup_bound)
            && !is_subtype_of_typed(sub, sup, class_loader)
        {
            return false;
        }
        return sub_inner
            .as_ref()
            .and_then(|inner| inner.base_name())
            .and_then(|name| class_loader(Fqn::new(name)))
            .is_none_or(|cls| cls.kind == kind);
    }

    // ── enum-string / trait-string <: class-string / its own kind ─
    // An enum name is a class-string, a trait name is not (the name
    // lattice keeps them apart the same way), and a kind-string only
    // satisfies its own kind.  What is left is lining up the bounds,
    // where a missing one is `object`.
    if let Some((sub_kind, sub_bound)) = class_kind_string(subtype)
        && let Some((kind_fits, sup_bound)) = match supertype.kind() {
            TypeKind::ClassString(bound) => Some((sub_kind == ClassLikeKind::Enum, bound.as_ref())),
            _ => class_kind_string(supertype).map(|(kind, bound)| (kind == sub_kind, bound)),
        }
    {
        if !kind_fits {
            return false;
        }
        let object_bound = PhpType::named(atom("object"));
        return is_subtype_of_typed(
            sub_bound.unwrap_or(&object_bound),
            sup_bound.unwrap_or(&object_bound),
            class_loader,
        );
    }

    // ── String literal <: model-property<Model> ────────────────
    // The Laravel PHPStan extensions' `model-property<Model>` is a string subtype
    // representing the property names of an Eloquent model.  A
    // string literal is a subtype only if it names a known
    // property.  When the model class cannot be loaded, stay
    // permissive (return true) to avoid false positives.
    if let TypeKind::Literal(lit) = subtype.kind()
        && lit.string_content().is_some()
        && let TypeKind::Generic(g) = supertype.kind()
        && g.name.eq_ignore_ascii_case("model-property")
        && g.args.len() == 1
    {
        let prop_name = lit.string_content().unwrap();
        if let Some(model_name) = g.args[0].base_name()
            && let Some(cls) = class_loader(Fqn::new(model_name))
        {
            let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                class_loader,
                crate::virtual_members::active_resolved_class_cache(),
            );
            return resolved.properties.iter().any(|p| *p.name == *prop_name)
                || crate::virtual_members::laravel::where_property::collect_column_names(
                    &resolved,
                )
                .iter()
                .any(|col| *col == *prop_name);
        }
        return true;
    }

    // ── String literal <: class-string<Bound> ────────────────────
    // A string literal that names an existing class satisfying the
    // bound is a valid `class-string<Bound>`, e.g. passing
    // `'RuntimeException'` where `class-string<Throwable>` is
    // expected.  Stay silent (return true) whenever the literal's
    // content cannot be resolved to a class — it may simply live in
    // a file we haven't indexed — and only reject when the resolved
    // class provably fails to satisfy the bound.
    if let Some(crate::php_type::LiteralValue::String(_)) = subtype.as_literal()
        && matches!(
            supertype.kind(),
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_)
        )
    {
        let TypeKind::Literal(lit) = subtype.kind() else {
            unreachable!()
        };
        let Some(class_name) = lit.string_content() else {
            return true;
        };
        let Some(cls) = class_loader(Fqn::new(&class_name)) else {
            return true;
        };
        if matches!(supertype.kind(), TypeKind::InterfaceString(_))
            && cls.kind != ClassLikeKind::Interface
        {
            return false;
        }
        return match supertype.kind() {
            TypeKind::ClassString(Some(bound)) | TypeKind::InterfaceString(Some(bound)) => {
                match bound.base_name() {
                    Some(bound_name) => is_subtype_of(&cls, bound_name, class_loader),
                    None => true,
                }
            }
            _ => true,
        };
    }

    // ── String literal <: enum-string / trait-string ────────────
    if let TypeKind::Literal(lit) = subtype.kind()
        && let Some(class_name) = lit.string_content()
        && let Some((kind, bound)) = class_kind_string(supertype)
    {
        let Some(cls) = class_loader(Fqn::new(&class_name)) else {
            return true;
        };
        return cls.kind == kind
            && bound
                .and_then(|bound| bound.base_name())
                .is_none_or(|bound_name| is_subtype_of(&cls, bound_name, class_loader));
    }

    // ── Class-like <: `object` / `iterable` ─────────────────────
    // Neither `object` nor `iterable` is a class name, so the nominal
    // walk below cannot speak for them: it needs a loadable name on both
    // sides.  `is_named_subtype` answers for a plain `Foo`, but not for a
    // `Foo<T>` — a generic carries the same instance, and its type
    // arguments have no say in whether it is an object.
    //
    // `iterable` is `array|Traversable`, so a class whose hierarchy
    // reaches `Traversable` satisfies it however it was spelled.
    if let Some(sub) = subtype.base_name()
        && let TypeKind::Named(sup) = supertype.kind()
    {
        if sup.eq_ignore_ascii_case("object") && crate::php_type::is_class_like_name(sub) {
            return true;
        }
        if sup.eq_ignore_ascii_case("iterable")
            && class_loader(Fqn::new(sub))
                .is_some_and(|cls| is_subtype_of(&cls, "Traversable", class_loader))
        {
            return true;
        }
    }

    // ── Nominal class hierarchy check ───────────────────────────
    // Both sides must resolve to a class name for the hierarchy walk.
    let sub_name = subtype.base_name();
    let sup_name = supertype.base_name();

    if let (Some(sub), Some(sup)) = (sub_name, sup_name) {
        // Try to load the subtype class and walk its hierarchy.
        if let Some(cls) = class_loader(Fqn::new(sub)) {
            return is_subtype_of(&cls, sup, class_loader);
        }
    }

    false
}

/// The class-like kind an `enum-string` or `trait-string` (bare, or with
/// a `<Bound>`) requires its name to have, and the bound if written.
fn class_kind_string(
    ty: &crate::php_type::PhpType,
) -> Option<(ClassLikeKind, Option<&crate::php_type::PhpType>)> {
    let (name, bound) = match ty.kind() {
        TypeKind::Named(name) => (name.as_str(), None),
        TypeKind::Generic(g) if g.args.len() == 1 => (g.name.as_str(), g.args.first()),
        _ => return None,
    };
    if name.eq_ignore_ascii_case("enum-string") {
        Some((ClassLikeKind::Enum, bound))
    } else if name.eq_ignore_ascii_case("trait-string") {
        Some((ClassLikeKind::Trait, bound))
    } else {
        None
    }
}

/// The runtime array key a shape key spelled `Foo::CONSTANT` evaluates to,
/// when `Foo` is loadable and the constant holds an integer or string
/// literal: `0` for `const NAME = 0;`, `name` for `const NAME = 'name';`.
///
/// `None` for any other key, and for a constant whose value is not one
/// such literal, which stays the `array-key` its spelling says.
pub(crate) fn evaluate_constant_shape_key(
    key: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<String> {
    let (class_name, constant_name) = crate::php_type::class_constant_key(key)?;
    let cls = class_loader(Fqn::new(class_name))?;
    let value = match cls.constants.iter().find(|c| c.name == constant_name) {
        Some(constant) => constant.value.clone(),
        // An inherited constant is only on the merged class.
        None => crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            class_loader,
            crate::virtual_members::active_resolved_class_cache(),
        )
        .constants
        .iter()
        .find(|c| c.name == constant_name)?
        .value
        .clone(),
    }?;
    let literal =
        crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(&value)?;
    match literal.as_literal()? {
        lit @ crate::php_type::LiteralValue::Int(_) => lit.parse_i64().map(|n| n.to_string()),
        lit @ crate::php_type::LiteralValue::String(_) => lit.string_content().map(Into::into),
        crate::php_type::LiteralValue::Float(_) => None,
    }
}

/// `ty` with the class-constant keys of its array shape replaced by the keys
/// they evaluate to, so that a shape spelled `array{Slots::NAME: string}`
/// compares as the `array{0: string}` it is at runtime.
///
/// `None` when `ty` is not a shape or none of its keys can be evaluated.
fn shape_with_constant_keys_evaluated(
    ty: &crate::php_type::PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<crate::php_type::PhpType> {
    use crate::php_type::{PhpType, ShapeEntry};

    let parts = ty.shape_parts()?;
    let mut evaluated_any = false;
    let entries: Vec<ShapeEntry> = parts
        .entries
        .iter()
        .map(|entry| {
            let key = entry.key.as_deref().and_then(|key| {
                let evaluated = evaluate_constant_shape_key(key, class_loader);
                evaluated_any |= evaluated.is_some();
                evaluated
            });
            ShapeEntry {
                key: key.or_else(|| entry.key.clone()),
                value_type: entry.value_type.clone(),
                optional: entry.optional,
            }
        })
        .collect();
    if !evaluated_any {
        return None;
    }
    let shape = if parts.is_list {
        PhpType::list_shape(entries)
    } else {
        PhpType::array_shape(entries)
    };
    Some(match parts.tail {
        Some((key, value)) => PhpType::unsealed_shape(shape, key.clone(), value.clone()),
        None => shape,
    })
}
