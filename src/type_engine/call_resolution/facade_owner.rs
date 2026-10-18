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
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
    backend: Option<&Backend>,
) -> Option<Arc<ClassInfo>> {
    if !class_has_method(owner, method_name, class_loader, cache) {
        return facade_accessor_concrete_owner(owner, method_name, class_loader, cache, backend);
    }

    // Real Laravel facades declare `getFacadeAccessor()` directly (never
    // inherited), so this raw lookup is a cheap way to rule out the
    // overwhelming majority of static calls that are not on a facade at
    // all, before paying for the resolution inside
    // `facade_accessor_concrete_owner` below.
    if owner.get_method_ci("getFacadeAccessor").is_none()
        || method_return_is_call_dependent(owner, method_name, class_loader, cache)
    {
        return None;
    }

    // A facade's own method may come from an `@method` docblock tag that
    // flattens the concrete class's argument-dependent return (e.g.
    // `Container::make()`'s `class-string<T>`-aware conditional return
    // becomes a bare `object|mixed` on the `App` facade's tag, and
    // `PendingRequest::async()`'s `self<T>` becomes a bare class name on
    // `Http`'s), or that drops the type arguments of a fixed one
    // (`Factory::recorded()`'s `Collection<int, array{…}>` becomes a bare
    // `Collection` on `Http`'s). Fall through to the concrete accessor only
    // when it actually supplies a return the facade's own tag lacks.
    let facade_return = method_return_type(owner, method_name, class_loader, cache);
    facade_accessor_concrete_owner(owner, method_name, class_loader, cache, backend).filter(
        |concrete| {
            method_return_is_call_dependent(concrete, method_name, class_loader, cache)
                || facade_return.as_ref().is_some_and(|facade_return| {
                    method_return_type(concrete, method_name, class_loader, cache).is_some_and(
                        |concrete_return| refines_bare_return(facade_return, &concrete_return),
                    )
                })
        },
    )
}

fn method_return_type(
    class_info: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Option<PhpType> {
    if let Some(method) = class_info.get_method_ci(method_name) {
        return method.return_type.clone();
    }
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class_info, class_loader, cache);
    merged.get_method_ci(method_name)?.return_type.clone()
}

/// Whether `concrete` is the same type as the bare `facade` return, with the
/// type arguments or shape the facade's tag left out: `Collection<int, User>`
/// for a bare `Collection`, or `list<string>` / `array{…}` for a bare
/// `array`.
fn refines_bare_return(facade: &PhpType, concrete: &PhpType) -> bool {
    match (facade.kind(), concrete.kind()) {
        (TypeKind::Nullable(facade), TypeKind::Nullable(concrete)) => {
            refines_bare_return(facade, concrete)
        }
        (TypeKind::Named(_), _) if facade.is_bare_array() => {
            concrete.is_array_like() && !concrete.is_bare_array()
        }
        (TypeKind::Named(facade_name), TypeKind::Generic(generic)) => {
            !generic.args.is_empty()
                && crate::util::short_name(facade_name)
                    .eq_ignore_ascii_case(crate::util::short_name(&generic.name))
        }
        _ => false,
    }
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

/// Whether a method's return type depends on the call rather than being
/// fixed by the signature: a PHPStan conditional return, or a return that
/// names one of the method's own `@template` parameters.
///
/// A facade's generated `@method` tag can express neither, so this is what
/// distinguishes a tag that merely restates the concrete method from one
/// that flattened it.
fn method_return_is_call_dependent(
    class_info: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> bool {
    if class_info
        .get_method_ci(method_name)
        .is_some_and(call_dependent_return)
    {
        return true;
    }
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class_info, class_loader, cache);
    if merged
        .get_method_ci(method_name)
        .is_some_and(call_dependent_return)
    {
        return true;
    }

    // A generic mixin's conditional may already have been collapsed from
    // its declared template default while being merged. Inspect the source
    // mixin as well so a facade's generated broad `@method` union does not
    // hide that more precise concrete-owner return type.
    class_info
        .mixins
        .iter()
        .chain(merged.mixins.iter())
        .any(|mixin| {
            class_loader(mixin.as_str()).is_some_and(|mixin_class| {
                mixin_class
                    .get_method_ci(method_name)
                    .is_some_and(call_dependent_return)
            })
        })
}

fn call_dependent_return(method: &crate::types::MethodInfo) -> bool {
    if method.conditional_return().is_some() {
        return true;
    }
    if method.template_params().is_empty() {
        return false;
    }
    let params: Vec<String> = method
        .template_params()
        .iter()
        .map(|p| p.to_string())
        .collect();
    method
        .return_type
        .as_ref()
        .is_some_and(|ret| ret.references_any_template_param(&params))
}

fn facade_accessor_concrete_owner(
    facade: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
    backend: Option<&Backend>,
) -> Option<Arc<ClassInfo>> {
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(facade, class_loader, cache);

    // Prefer the accessor recorded at parse time: body-return inference
    // honours the declared `: string` return type rather than the
    // `::class` value we need here.
    if let Some(concrete) = facade
        .laravel()
        .and_then(|l| l.facade_accessor)
        .and_then(|accessor| facade_accessor_to_class_name(accessor, backend))
        .and_then(|name| class_loader(&name))
        .filter(|class| class_has_method(class, method_name, class_loader, cache))
    {
        return Some(concrete);
    }

    if let Some(accessor) = facade
        .get_method_ci("getFacadeAccessor")
        .or_else(|| merged.get_method_ci("getFacadeAccessor"))
        && let Some(backend) = backend
        && let Some(inferred) = crate::type_engine::call_resolution::try_infer_body_return_type(
            backend,
            &facade.fqn(),
            accessor,
            // A facade accessor takes no arguments, so there is nothing
            // a call site could decide about it.
            &[],
        )
        && let Some(concrete_name) = facade_accessor_class_name(&inferred, Some(backend))
        && let Some(concrete) = class_loader(&concrete_name)
        && class_has_method(&concrete, method_name, class_loader, cache)
    {
        return Some(concrete);
    }

    facade
        .mixins
        .iter()
        .chain(merged.mixins.iter())
        .find_map(|mixin| {
            let class = class_loader(mixin.as_str())?;
            class_has_method(&class, method_name, class_loader, cache).then_some(class)
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
            backend?.container_alias_concrete_fqn(&key)
        }
        _ => None,
    }
}

fn facade_accessor_to_class_name(
    accessor: crate::types::FacadeAccessor,
    backend: Option<&Backend>,
) -> Option<String> {
    match accessor {
        crate::types::FacadeAccessor::Class(name) => Some(name.to_string()),
        crate::types::FacadeAccessor::Alias(key) => {
            backend?.container_alias_concrete_fqn(key.as_str())
        }
    }
}
