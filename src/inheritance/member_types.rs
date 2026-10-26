//! Reading one member's type through the inheritance chain.

use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;

/// Look up a method's return type through the inheritance chain.
///
/// Resolves inheritance for `class`, finds the method named
/// `method_name`, and returns its `return_type`.  This is a
/// convenience wrapper around [`resolve_class_fully`](crate::virtual_members::resolve_class_fully)
/// that eliminates the repeated merge → find → extract pattern
/// used across many modules.
///
/// Uses full resolution (base inheritance + virtual member providers)
/// so that virtual methods from `@method` tags, `@mixin` classes,
/// and framework providers are included.
pub(crate) fn resolve_method_return_type(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    // Try the class directly first — it may already be fully resolved
    // with generic substitutions applied.  Falling through to the cache
    // would return the un-substituted base class (keyed by bare FQN),
    // losing template parameter substitutions like TModel → Product.
    //
    // A declaration that states no return type at all is not an answer,
    // though: an override written `public function getFileName()` with
    // `{@inheritDoc}` above it says nothing, and what it means is what the
    // ancestor said.  That type lives on the merged class, so fall through
    // to it rather than reporting the override's silence as "unknown".
    if let Some(m) = class.get_method(method_name)
        && m.return_type.is_some()
    {
        return m.return_type.clone();
    }
    let cache = crate::virtual_members::active_resolved_class_cache();
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class, class_loader, cache);
    merged
        .methods
        .iter()
        .find(|m| m.name == method_name)
        .and_then(|m| m.return_type.clone())
}

/// Look up a property's type hint through the inheritance chain.
///
/// Resolves inheritance for `class`, finds the property named
/// `prop_name`, and returns its `type_hint`.  This is a
/// convenience wrapper around [`resolve_class_fully`](crate::virtual_members::resolve_class_fully)
/// that eliminates the repeated merge → find → extract pattern
/// used across many modules.
///
/// Uses full resolution (base inheritance + virtual member providers)
/// so that virtual properties from `@property` tags, `@mixin` classes,
/// and framework providers are included.
pub(crate) fn resolve_property_type_hint(
    class: &ClassInfo,
    prop_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    // Try the class directly first — it may already have the property
    // with generic substitutions applied.
    if let Some(p) = class.properties.iter().find(|p| p.name == prop_name)
        && p.type_hint.is_some()
    {
        let hint = p.type_hint.clone().unwrap();
        return Some(replace_self_in_property_type(hint, class));
    }
    let cache = crate::virtual_members::active_resolved_class_cache();
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class, class_loader, cache);
    if let Some(hint) = merged
        .properties
        .iter()
        .find(|p| p.name == prop_name)
        .and_then(|p| p.type_hint.clone())
    {
        return Some(replace_self_in_property_type(hint, class));
    }

    // Eloquent relation properties resolve case-insensitively: access like
    // `$model->orderproducts` flows through `__get()` → `isRelation()` →
    // `method_exists()`, so a relation-backed virtual property matches the
    // relationship regardless of the case used at the access site.
    if crate::virtual_members::laravel::class_has_relation_method_ci(&merged, prop_name)
        && let Some(hint) = merged
            .properties
            .iter()
            .find(|p| p.is_virtual && p.name.eq_ignore_ascii_case(prop_name))
            .and_then(|p| p.type_hint.clone())
    {
        return Some(replace_self_in_property_type(hint, class));
    }

    // Fallback: if the class has a `__get` method with method-level
    // template parameters and an IndexAccess return type (e.g.
    // `@template K as key-of<TData>` / `@return TData[K]`), infer K
    // from the property name and evaluate the indexed access.
    // Try the original class first — it may already carry generic
    // substitutions (e.g. from `apply_generic_args`) so `__get`'s
    // return type is already concrete.
    if let Some(ty) = resolve_magic_get_return_type(class, prop_name) {
        return Some(ty);
    }
    resolve_magic_get_return_type(&merged, prop_name)
}

/// Replace `self`/`static`/`$this` references in a property type with
/// the owning class's fully qualified name.
///
/// Skips replacement for synthetic classes (like `__object_shape`) where
/// `self` refers to the caller's context, not the synthetic class itself.
fn replace_self_in_property_type(ty: PhpType, class: &ClassInfo) -> PhpType {
    if ty.contains_self_ref() && !class.name.starts_with("__") {
        ty.replace_self(&class.fqn())
    } else {
        ty
    }
}

/// Resolve the return type of a property access through a `__get` magic
/// method whose return type indexes a shape by the accessed property name.
///
/// For example, given `@return array{a: int, b: string}[K]` on `__get`
/// with a method-level `@template K`, accessing `$obj->a` infers `K = 'a'`
/// from the property name and evaluates the index access to `int`.
fn resolve_magic_get_return_type(class: &ClassInfo, prop_name: &str) -> Option<PhpType> {
    let get_method = class
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("__get"))?;

    let return_type = get_method.return_type.as_ref()?;

    // When __get has no template params, return the declared return type
    // directly (with self/static resolved to the owning class).
    if get_method.template_params().is_empty() {
        let resolved = if return_type.contains_self_ref() {
            return_type.replace_self(&class.fqn())
        } else {
            return_type.clone()
        };
        return Some(resolved);
    }

    // Build a substitution map: for each method-level template parameter,
    // try to infer its value from the property name being accessed.
    let mut method_subs = std::collections::HashMap::new();
    for tparam in get_method.template_params() {
        // The template param is typically bounded by key-of<SomeShape>.
        // After class-level substitution the bound is already concrete
        // (e.g. key-of<array{a: int, b: string}> → 'a'|'b').
        // We infer the template value as a literal string matching the
        // property name.
        method_subs.insert(tparam.to_string(), PhpType::literal_string_value(prop_name));
    }

    let resolved = return_type.substitute(&method_subs);

    // Only return if the substitution actually resolved to something
    // concrete (not still an IndexAccess with an unresolved key).
    if matches!(&resolved.kind(), TypeKind::IndexAccess(_, _)) {
        return None;
    }

    Some(resolved)
}
