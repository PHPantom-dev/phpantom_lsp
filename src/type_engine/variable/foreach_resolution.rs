/// Iterable element/key type extraction from a class's generic
/// annotations (`@extends Collection<int, User>`,
/// `@implements IteratorAggregate<int, User>`), plus
/// [`resolve_expression_type`], a thin wrapper over the unified RHS
/// pipeline.
///
/// Consumed by the forward walker's foreach/destructuring handling,
/// array-access resolution, and raw type inference.
use std::collections::HashMap;
use std::sync::Arc;

use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};
use crate::util::short_name;

use crate::type_engine::resolver::VarResolutionCtx;

/// Resolve an expression's structured type via the unified pipeline.
///
/// Wraps `resolve_rhs_expression` + `types_joined` into a single
/// `Option<PhpType>`.  Returns `None` when the unified pipeline
/// produces no results or an empty type string.
pub(crate) fn resolve_expression_type<'b>(
    expr: &'b mago_syntax::cst::Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolved = super::rhs_resolution::resolve_rhs_expression(expr, ctx);
    if resolved.is_empty() {
        return None;
    }
    Some(ResolvedType::types_joined(&resolved))
}

/// The context pieces deriving an iterable's element and key types needs.
///
/// The derivation is asked for from contexts of different shapes — the
/// forward walker binding a `foreach`, and Blade's `@each` call-site
/// inference asking what one entry of a collection is — so it reads the
/// four pieces both carry rather than either whole context.
pub(crate) struct IterableCtx<'a> {
    pub current_class: &'a ClassInfo,
    pub all_classes: &'a [Arc<ClassInfo>],
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    pub resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
}

impl<'a> IterableCtx<'a> {
    pub(crate) fn from_var_ctx(ctx: &'a VarResolutionCtx<'_>) -> Self {
        Self {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            class_loader: ctx.class_loader,
            resolved_class_cache: ctx.resolved_class_cache,
        }
    }
}

/// The type `foreach ($expr as $value)` binds, given the iterable's own
/// resolved type.
///
/// Tries the type's own generic parameters (or, for a tuple-style shape,
/// the union of its positional values), then the generics of the class it
/// names, then each member of a union individually.
pub(crate) fn iteration_value_type(iter_type: &PhpType, ctx: &IterableCtx<'_>) -> Option<PhpType> {
    if let Some(vt) = iter_type.iterable_element_type() {
        return Some(vt);
    }
    if let Some(et) = resolve_iterable_element_via_class(iter_type, ctx)
        && !is_unsubstituted_template_param(&et)
    {
        return Some(et);
    }
    if let TypeKind::Union(members) = iter_type.kind() {
        for member in members {
            if let Some(vt) = member.extract_value_type(false) {
                return Some(vt.clone());
            }
            if let Some(et) = resolve_iterable_element_via_class(member, ctx)
                && !is_unsubstituted_template_param(&et)
            {
                return Some(et);
            }
        }
    }
    None
}

/// The type `foreach ($expr as $key => $value)` binds `$key` to, given
/// the iterable's own resolved type.
///
/// `None` only when nothing about the value resolves to an iterable at
/// all; a type that resolves but names no key type answers with the whole
/// key domain PHP allows, and the caller falls back to the same.
pub(crate) fn iteration_key_type(iter_type: &PhpType, ctx: &IterableCtx<'_>) -> Option<PhpType> {
    if let Some(kt) = key_domain(iter_type) {
        // Benevolent when part of the domain is ours rather than the
        // array's: holding a `substr($key, …)` to the `int` branch of a
        // union we invented is a false positive.  `is_type_compatible`
        // implements that half; the union still narrows like any other.
        return Some(if iter_type.has_open_key_domain() {
            PhpType::benevolent(kt)
        } else {
            kt
        });
    }
    let key_type = resolve_iterable_key_via_class(iter_type, ctx)?;
    (!is_unsubstituted_template_param(&key_type)).then_some(key_type)
}

/// The key domain of an iterable, widening the shapes that name only a
/// value type to every key PHP allows.
///
/// [`PhpType::iterable_key_type`] answers `int` for `array<T>`, `T[]` and
/// bare `array`, which is the useful guess when reading one element out of
/// a sequential array but wrong for a `foreach`: an array whose docblock
/// says nothing about its keys can hand out string keys just as well, and
/// binding only the `int` half reports every one of them as a mismatch.
/// A `list<T>` is not open — it promises `int` keys — and a spelled-out
/// key type is reported verbatim.
fn key_domain(ty: &PhpType) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Nullable(inner) => key_domain(inner),
        // Member-wise, so a union that mixes an open member with a
        // spelled-out one keeps what the latter said.
        TypeKind::Union(members) => {
            let keys: Vec<PhpType> = members.iter().filter_map(key_domain).collect();
            match keys.len() {
                0 => None,
                1 => keys.into_iter().next(),
                _ => Some(PhpType::join_runtime_value_types(keys)),
            }
        }
        _ if ty.has_open_key_domain() => {
            Some(PhpType::union(vec![PhpType::int(), PhpType::string()]))
        }
        _ => ty.iterable_key_type(),
    }
}

/// Resolve the element type of an iterable via class inheritance.
///
/// When the iterable type is a bare class name (e.g. `OrderProductCollection`),
/// this resolves it to `ClassInfo`, merges the full inheritance chain, and
/// extracts the element type from `@extends` / `@implements` generics using
/// [`extract_iterable_element_type_from_class`].
pub(crate) fn resolve_iterable_element_via_class(
    iter_type: &PhpType,
    ctx: &IterableCtx<'_>,
) -> Option<PhpType> {
    // Accept bare class names, whether or not wrapped in `Nullable` (e.g.
    // `?SimpleXMLElement`, the return type of `SimpleXMLElement::children()`).
    // `base_name` unwraps `Nullable`/`Generic` to the underlying class name.
    // Bare generic types like `Collection<int, User>` are handled by the
    // caller's own generic extraction, so this only needs the name for the
    // `class_loader` fallback below; `type_hint_to_classes_typed` handles
    // the full (possibly nullable) type itself.
    let class_name = iter_type.base_name()?;

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        iter_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );

    if classes.is_empty() {
        // Try direct class loader as fallback (handles FQN names).
        let cls = (ctx.class_loader)(class_name)?;
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        return extract_iterable_element_type_from_class(&merged, ctx.class_loader);
    }

    for cls in &classes {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let element_type = extract_iterable_element_type_from_class(&merged, ctx.class_loader);
        if let Some(ref et) = element_type {
            // When the extracted type is an unsubstituted template parameter
            // (e.g. `TModel`), resolve it through the class's template bounds
            // (e.g. `@template TModel of BlogAuthor` → `BlogAuthor`).
            if let Some(bound) = template_bound(et, &merged) {
                return Some(bound);
            }
            return element_type;
        }
    }

    None
}

/// Resolve the iterable **key** type from a class's `implements_generics`
/// / `extends_generics`.  Mirrors [`resolve_iterable_element_via_class`].
pub(crate) fn resolve_iterable_key_via_class(
    iter_type: &PhpType,
    ctx: &IterableCtx<'_>,
) -> Option<PhpType> {
    // See `resolve_iterable_element_via_class`: unwrap `Nullable`/`Generic`
    // via `base_name` so `?SimpleXMLElement`-style iterable types resolve.
    let class_name = iter_type.base_name()?;

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        iter_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );

    if classes.is_empty() {
        let cls = (ctx.class_loader)(class_name)?;
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        return extract_iterable_key_type_from_class(&merged, ctx.class_loader);
    }

    for cls in &classes {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let key_type = extract_iterable_key_type_from_class(&merged, ctx.class_loader);
        if let Some(ref kt) = key_type {
            if let Some(bound) = template_bound(kt, &merged) {
                return Some(bound);
            }
            return key_type;
        }
    }

    None
}

/// The bound a class declares for `ty`, when `ty` is one of the class's
/// own template parameters (`@template TModel of BlogAuthor` → `BlogAuthor`).
fn template_bound(ty: &PhpType, class: &ClassInfo) -> Option<PhpType> {
    let name = ty.base_name()?;
    if !class
        .template_params
        .iter()
        .any(|p| p.as_ref() as &str == name)
    {
        return None;
    }
    class
        .template_param_bounds
        .get(&crate::atom::atom(name))
        .cloned()
}

/// Check whether a `PhpType` looks like an unsubstituted template
/// parameter (e.g. `TValue`, `TKey`, `TModel`).  These are bare named
/// types whose name starts with `T` followed by an uppercase letter
/// and are not known PHP built-in types.
pub(crate) fn is_unsubstituted_template_param(ty: &PhpType) -> bool {
    let name = match ty.kind() {
        TypeKind::Named(n) => n.as_str(),
        _ => return false,
    };
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'T' && bytes[1].is_ascii_uppercase()
}

/// Known interface/class names whose generic parameters describe
/// iteration types in PHP's `foreach`.
const ITERABLE_IFACE_NAMES: &[&str] = &[
    "Iterator",
    "IteratorAggregate",
    "Traversable",
    "ArrayAccess",
    "Enumerable",
];

/// Extract the iterable **value** (element) type from a class's generic
/// annotations.
///
/// When a collection class like `UserCollection` has
/// `@extends Collection<int, User>` or
/// `@implements IteratorAggregate<int, User>`, this
/// function returns `Some("User")`.
///
/// Checks (in order of priority):
/// 1. `implements_generics` for known iterable interfaces
/// 2. `extends_generics` for any parent with generic type args
///
/// Returns `None` when no generic iterable annotation is found or
/// when the element type is a scalar (scalars have no completable
/// members).
pub(crate) fn extract_iterable_element_type_from_class(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    // 1. Check implements_generics for known iterable interfaces.
    for (name, args) in &class.implements_generics {
        let short = short_name(name);
        if ITERABLE_IFACE_NAMES.contains(&short)
            && let Some(arg) = iterable_value_arg(args)
        {
            let value = resolve_own_template_arg(arg, class);
            if !is_unbounded_template_placeholder(&value) {
                return Some(value);
            }
        }
    }

    // 1b. Check implements_generics for interfaces that transitively
    //     extend a known iterable interface (e.g. `TypedCollection`
    //     extends `IteratorAggregate`).
    for (name, args) in &class.implements_generics {
        let short = short_name(name);
        if !ITERABLE_IFACE_NAMES.contains(&short)
            && let Some(arg) = iterable_value_arg(args)
            && let Some(iface) = class_loader(name)
            && is_transitive_iterable(&iface, class_loader)
        {
            let value = resolve_own_template_arg(arg, class);
            if !is_unbounded_template_placeholder(&value) {
                return Some(value);
            }
        }
    }

    // 2. Check extends_generics — common for collection subclasses
    //    like `@extends Collection<int, User>`.
    for (_, args) in &class.extends_generics {
        if let Some(arg) = iterable_value_arg(args) {
            let value = resolve_own_template_arg(arg, class);
            if !is_unbounded_template_placeholder(&value) {
                return Some(value);
            }
        }
    }

    // 3. Fall back to the `current()` return type when the class
    //    implements `Iterator` directly (not `IteratorAggregate`) without
    //    a generic annotation. `SimpleXMLElement` is the prototypical
    //    example: it implements `Iterator` with `current(): static`, so
    //    iterating it yields instances of the iterated class itself.
    if class_directly_implements(class, class_loader, "Iterator")
        && let Some(method) = class.get_method("current")
        && let Some(return_type) = &method.return_type
    {
        return Some(return_type.replace_self(&class.fqn()));
    }

    // 4. Fall back to the `offsetGet()` return type when the class
    //    implements `ArrayAccess` directly without a usable generic
    //    annotation (e.g. an unbound `@template` self-reference, or no
    //    docblock generics at all). Mirrors the `current()` fallback
    //    above: `$obj[$k]` invokes `offsetGet`, so its declared return
    //    type is the most precise answer available.
    if class_directly_implements(class, class_loader, "ArrayAccess")
        && let Some(method) = class.get_method("offsetGet")
        && let Some(return_type) = &method.return_type
    {
        return Some(return_type.replace_self(&class.fqn()));
    }

    None
}

/// Select the generic argument that describes the iterated **value** type.
///
/// Iterable generics follow the `<TKey, TValue>` convention, so the value
/// is the *second* argument whenever two or more are present. This matters
/// for the SPL wrapper iterators
/// (`IteratorIterator`/`FilterIterator`/`AppendIterator`), which add a
/// third `TIterator` argument: `@extends FilterIterator<int, SplFileInfo,
/// \Iterator<int, SplFileInfo>>` has its value type (`SplFileInfo`) in the
/// middle, not last. With a single argument (e.g. `IteratorAggregate<User>`)
/// that lone argument is the value. Returns `None` for an empty list.
fn iterable_value_arg(args: &[PhpType]) -> Option<&PhpType> {
    if args.len() >= 2 {
        Some(&args[1])
    } else {
        args.last()
    }
}

/// Resolve a generic argument that references the class's own `@template`
/// parameter (e.g. `T` in `@implements ArrayAccess<int, T>` declared on a
/// class with `@template T of SomeBound`) to its upper bound.
///
/// `implements_generics` / `extends_generics` store a class's generic
/// annotations exactly as written; when an annotation references the same
/// class's own template parameter (rather than a concrete type or a
/// parent's template parameter, which are substituted elsewhere), nothing
/// else resolves it. Without this, the raw template name (e.g. `"T"`)
/// would leak through as if it were a real, unrelated class name.
fn resolve_own_template_arg(value: &PhpType, class: &ClassInfo) -> PhpType {
    if class.template_params.is_empty() {
        return value.clone();
    }
    let subs: HashMap<String, PhpType> = class
        .template_params
        .iter()
        .map(|param| {
            let bound = class
                .template_param_bounds
                .get(param)
                .cloned()
                .unwrap_or_else(PhpType::mixed);
            (param.to_string(), bound)
        })
        .collect();
    value.substitute(&subs)
}

/// Check whether a generic argument is an unbounded template parameter
/// that was substituted with `mixed` as a fallback (no explicit
/// `@implements`/`@extends` generic annotation was given).
///
/// Interfaces like `Iterator<TKey, TValue>` propagate their template
/// params through `@template-extends Traversable<TKey, TValue>` even when
/// the implementing class never annotates concrete types; in that case
/// the merge falls back to substituting each param with `mixed` (see
/// `resolve_class_fully_inner` in `virtual_members/resolve.rs`). Treating
/// that placeholder as a "found" element type would shadow the callers'
/// more precise `current()`/`key()` fallbacks.
fn is_unbounded_template_placeholder(ty: &PhpType) -> bool {
    matches!(ty.kind(), TypeKind::Named(name) if name.eq_ignore_ascii_case("mixed"))
}

/// Check whether `class`, or an ancestor reached by walking the `extends`
/// chain, implements `<iface_name>` — either directly or through an
/// interface that transitively extends it.
///
/// The transitive check matters for SPL classes like `DirectoryIterator`,
/// which declare `implements SeekableIterator` (and `SeekableIterator`
/// extends `Iterator`) rather than naming `Iterator` outright. Without it,
/// the callers' `current()`/`key()` fallbacks never fire for such classes.
fn class_directly_implements(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    iface_name: &str,
) -> bool {
    let implements_here = |class: &ClassInfo| {
        class.interfaces.iter().any(|i| {
            short_name(i).eq_ignore_ascii_case(iface_name)
                || interface_extends_named(i, class_loader, iface_name)
        })
    };

    if implements_here(class) {
        return true;
    }

    let mut visited = std::collections::HashSet::new();
    visited.insert(class.name.to_string());
    let mut parent_name = class.parent_class.as_ref().map(|a| a.to_string());
    while let Some(name) = parent_name {
        if !visited.insert(name.clone()) {
            break;
        }
        let Some(parent) = class_loader(&name) else {
            break;
        };
        if implements_here(&parent) {
            return true;
        }
        parent_name = parent.parent_class.as_ref().map(|a| a.to_string());
    }
    false
}

/// Check whether the interface named `iface_name` transitively extends the
/// interface `target` by walking its interface-extends chain.
fn interface_extends_named(
    iface_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    target: &str,
) -> bool {
    let mut visited = std::collections::HashSet::new();
    interface_extends_named_inner(iface_name, class_loader, target, &mut visited)
}

fn interface_extends_named_inner(
    iface_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    target: &str,
    visited: &mut std::collections::HashSet<String>,
) -> bool {
    if !visited.insert(iface_name.to_string()) {
        return false;
    }
    let Some(iface) = class_loader(iface_name) else {
        return false;
    };
    // Interfaces record the interfaces they extend in `interfaces`, and
    // (for interface-extends-interface with generics) also in
    // `extends_generics`. Interface `parent_class` covers a rare additional
    // extends form. Check every parent name for a match or recurse.
    let parents = iface
        .interfaces
        .iter()
        .map(|i| i.to_string())
        .chain(iface.extends_generics.iter().map(|(n, _)| n.to_string()))
        .chain(iface.parent_class.iter().map(|p| p.to_string()));
    for parent in parents {
        if short_name(&parent).eq_ignore_ascii_case(target) {
            return true;
        }
        if interface_extends_named_inner(&parent, class_loader, target, visited) {
            return true;
        }
    }
    false
}

/// Extract the iterable **key** type from a class's generic annotations.
///
/// Mirrors `extract_iterable_element_type_from_class` but returns the
/// first generic parameter (key) instead of the value argument.  Only
/// returns a key type when the iterable interface has 2+ generic
/// parameters (so `list<User>` returns `None` → fallback to `int`).
pub(in crate::type_engine) fn extract_iterable_key_type_from_class(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    // 1. Check implements_generics for known iterable interfaces.
    for (name, args) in &class.implements_generics {
        let short = short_name(name);
        if ITERABLE_IFACE_NAMES.contains(&short) && args.len() >= 2 {
            let key = resolve_own_template_arg(&args[0], class);
            if !is_unbounded_template_placeholder(&key) {
                return Some(key);
            }
        }
    }

    // 1b. Transitive iterable interfaces.
    for (name, args) in &class.implements_generics {
        let short = short_name(name);
        if !ITERABLE_IFACE_NAMES.contains(&short)
            && args.len() >= 2
            && let Some(iface) = class_loader(name)
            && is_transitive_iterable(&iface, class_loader)
        {
            let key = resolve_own_template_arg(&args[0], class);
            if !is_unbounded_template_placeholder(&key) {
                return Some(key);
            }
        }
    }

    // 2. Check extends_generics.
    for (_, args) in &class.extends_generics {
        if args.len() >= 2 {
            let key = resolve_own_template_arg(&args[0], class);
            if !is_unbounded_template_placeholder(&key) {
                return Some(key);
            }
        }
    }

    // 3. Fall back to the `key()` return type when the class implements
    //    `Iterator` directly without a generic annotation. Mirrors the
    //    `current()` fallback in `extract_iterable_element_type_from_class`.
    if class_directly_implements(class, class_loader, "Iterator")
        && let Some(method) = class.get_method("key")
        && let Some(return_type) = &method.return_type
    {
        return Some(return_type.replace_self(&class.fqn()));
    }

    None
}

/// Check whether an interface transitively extends a known iterable
/// interface (e.g. `TypedCollection extends IteratorAggregate`).
fn is_transitive_iterable(
    iface: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let mut visited = std::collections::HashSet::new();
    is_transitive_iterable_inner(iface, class_loader, &mut visited)
}

fn is_transitive_iterable_inner(
    iface: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut std::collections::HashSet<String>,
) -> bool {
    // Recurse through a parent name, guarding against cyclic hierarchies.
    let recurse = |name: &str, visited: &mut std::collections::HashSet<String>| -> bool {
        if !visited.insert(name.to_string()) {
            return false;
        }
        class_loader(name)
            .is_some_and(|parent| is_transitive_iterable_inner(&parent, class_loader, visited))
    };

    // Check direct interfaces, then recurse into any that are not
    // themselves a known iterable so a two-hop ancestor is still found.
    for parent in &iface.interfaces {
        if ITERABLE_IFACE_NAMES.contains(&short_name(parent)) {
            return true;
        }
        if recurse(parent, visited) {
            return true;
        }
    }
    // Check extends_generics for the interface-extends-interface pattern.
    for (name, _) in &iface.extends_generics {
        if ITERABLE_IFACE_NAMES.contains(&short_name(name)) {
            return true;
        }
        if recurse(name, visited) {
            return true;
        }
    }
    // Check parent class (interfaces use `parent_class` for extends).
    if let Some(ref parent_name) = iface.parent_class {
        if ITERABLE_IFACE_NAMES.contains(&short_name(parent_name)) {
            return true;
        }
        if recurse(parent_name, visited) {
            return true;
        }
    }
    false
}
