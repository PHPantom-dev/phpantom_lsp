//! Resolving a static call on a Laravel facade to the concrete class that
//! actually implements the called method.
//!
//! A facade forwards every static call to the container instance named by
//! its `getFacadeAccessor()`.  The facade class itself declares the method
//! either not at all (`__callStatic` handles it) or through an `@method`
//! docblock tag that flattens the concrete class's argument-dependent
//! return type.  Both cases need the concrete class to type the call, so
//! the selection below is shared by the assignment path and the chain
//! resolver.

use std::sync::Arc;

use crate::Backend;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;
use crate::virtual_members::ResolvedClassCache;

/// The class that should type a `Facade::method(…)` call, or `None` when
/// the static call is not a facade forward (the overwhelming majority) and
/// the declared owner already types it.
pub(crate) fn facade_concrete_owner(
    owner: &ClassInfo,
    method_name: &str,
    content: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
    backend: Option<&Backend>,
) -> Option<ClassInfo> {
    if !class_has_method(owner, method_name, class_loader, cache) {
        return facade_accessor_concrete_owner(
            owner,
            method_name,
            content,
            class_loader,
            cache,
            backend,
        );
    }

    // Real Laravel facades declare `getFacadeAccessor()` directly (never
    // inherited), so this raw lookup is a cheap way to rule out the
    // overwhelming majority of static calls that are not on a facade at
    // all, before paying for the source re-parse inside
    // `facade_accessor_concrete_owner` below.
    if owner.get_method_ci("getFacadeAccessor").is_none()
        || method_has_conditional_return(owner, method_name, class_loader, cache)
    {
        return None;
    }

    // A facade's own method may come from an `@method` docblock tag that
    // flattens the concrete class's argument-dependent return (e.g.
    // `Container::make()`'s `class-string<T>`-aware conditional return
    // becomes a bare `object|mixed` on the `App` facade's tag). Fall
    // through to the concrete accessor only when it actually supplies a
    // conditional return the facade's own tag does not.
    facade_accessor_concrete_owner(owner, method_name, content, class_loader, cache, backend)
        .filter(|concrete| {
            method_has_conditional_return(concrete, method_name, class_loader, cache)
        })
}

fn class_has_method(
    class_info: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> bool {
    if class_info.get_method_ci(method_name).is_some() {
        return true;
    }
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class_info, class_loader, cache);
    merged.get_method_ci(method_name).is_some()
}

fn method_has_conditional_return(
    class_info: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> bool {
    if class_info
        .get_method_ci(method_name)
        .is_some_and(|m| m.conditional_return.is_some())
    {
        return true;
    }
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class_info, class_loader, cache);
    merged
        .get_method_ci(method_name)
        .is_some_and(|m| m.conditional_return.is_some())
}

fn facade_accessor_concrete_owner(
    facade: &ClassInfo,
    method_name: &str,
    content: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
    backend: Option<&Backend>,
) -> Option<ClassInfo> {
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(facade, class_loader, cache);

    // When the facade is declared in the current file, read its
    // `getFacadeAccessor()` return directly from source: body-return
    // inference honours the declared `: string` return type rather than
    // the `::class` value we need here. Scope the parse to the facade's
    // own class so a file declaring several facades does not
    // cross-resolve.
    if let Some(concrete) =
        crate::virtual_members::laravel::parse_facade_accessor_for_class(content, &facade.name)
            .and_then(|accessor| facade_accessor_to_class_name(accessor, backend))
            .and_then(|name| class_loader(&name))
            .filter(|class| class_has_method(class, method_name, class_loader, cache))
    {
        return Some(Arc::unwrap_or_clone(concrete));
    }

    if let Some(accessor) = facade
        .get_method_ci("getFacadeAccessor")
        .or_else(|| merged.get_method_ci("getFacadeAccessor"))
        && let Some(backend) = backend
        && let Some(inferred) = crate::type_engine::call_resolution::try_infer_body_return_type(
            backend,
            &facade.fqn(),
            accessor,
        )
        && let Some(concrete_name) = facade_accessor_class_name(&inferred, Some(backend))
        && let Some(concrete) = class_loader(&concrete_name)
        && class_has_method(&concrete, method_name, class_loader, cache)
    {
        return Some(Arc::unwrap_or_clone(concrete));
    }

    facade
        .mixins
        .iter()
        .chain(merged.mixins.iter())
        .find_map(|mixin| {
            let class = class_loader(mixin.as_str())?;
            class_has_method(&class, method_name, class_loader, cache)
                .then(|| Arc::unwrap_or_clone(class))
        })
}

fn facade_accessor_class_name(ty: &PhpType, backend: Option<&Backend>) -> Option<String> {
    match ty.kind() {
        TypeKind::ClassString(Some(inner)) => inner.base_name().map(ToString::to_string),
        TypeKind::Named(name) => Some(name.to_string()),
        // `getFacadeAccessor()` returning a bare string literal (`'app'`) is
        // a container-binding key, not a class name; resolve it through the
        // core container alias table, same as the `::class` case above.
        TypeKind::Literal(value) => {
            let key = value.string_content()?;
            backend?.container_alias_concrete_fqn(key)
        }
        _ => None,
    }
}

fn facade_accessor_to_class_name(
    accessor: crate::virtual_members::laravel::FacadeAccessor,
    backend: Option<&Backend>,
) -> Option<String> {
    match accessor {
        crate::virtual_members::laravel::FacadeAccessor::Class(name) => Some(name),
        crate::virtual_members::laravel::FacadeAccessor::Alias(key) => {
            backend?.container_alias_concrete_fqn(&key)
        }
    }
}
