//! Facade-to-concrete static forwarding.
//!
//! `Facade::__callStatic()` resolves the container binding named by
//! `getFacadeAccessor()` and forwards the call to that instance, so every
//! public instance method of the concrete class is callable statically on
//! the facade.  Laravel's own facades spell that out in a generated
//! `@method static` docblock, but an app-defined or package facade that
//! never ran `facade-documenter` has nothing, and listing members on it
//! would otherwise turn up only `__callStatic`.
//!
//! This provider converts the concrete class's public instance methods
//! into static virtual methods on the facade.  Return types that name the
//! concrete instance (`static`, `$this`, `self`) become the concrete class
//! so a fluent chain continues on it rather than on the facade.
//!
//! The accessor names the concrete class either directly
//! (`return Container::class;`) or through a container binding
//! (`return 'sentry';`), which the alias table on the resolved-class cache
//! maps to the class the framework or a service provider bound it to.
//!
//! The provider runs after the PHPDoc provider, so a facade that *does*
//! carry `@method static` tags keeps them: the generated tags flatten
//! argument-dependent returns deliberately, and matching Laravel's own
//! published signatures beats second-guessing them here.

use std::borrow::Cow;
use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::{ClassInfo, FacadeAccessor, MethodInfo, Visibility};
use crate::virtual_members::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};

use super::helpers::walks_parent_chain;

/// The fully-qualified name of the base facade class.
const FACADE_FQN: &str = "Illuminate\\Support\\Facades\\Facade";

/// Virtual member provider for Laravel facades.
///
/// Applies to a class that extends `Illuminate\Support\Facades\Facade`
/// and declares a `getFacadeAccessor()` naming a concrete class.
pub struct LaravelFacadeProvider;

impl VirtualMemberProvider for LaravelFacadeProvider {
    fn applies_to(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        // The accessor check comes first: it is a field read, and it is
        // `None` for every class in the project bar the facades, so the
        // parent-chain walk below only runs for those.
        has_accessor(class) && walks_parent_chain(class, class_loader, is_facade)
    }

    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        VirtualMembers {
            methods: build_forwarded_methods(class, class_loader, cache),
            properties: Vec::new(),
            constants: Vec::new(),
        }
    }
}

fn is_facade(class_name: &str) -> bool {
    class_name == FACADE_FQN
}

/// Whether the class declares a `getFacadeAccessor()` we could read at
/// parse time, in either of its two shapes.
fn has_accessor(class: &ClassInfo) -> bool {
    class
        .laravel()
        .is_some_and(|laravel| laravel.facade_accessor.is_some())
}

/// The concrete class a facade's `getFacadeAccessor()` names.
///
/// An accessor that names a class (`return Container::class;`) is the name
/// itself.  One that names a container binding (`return 'view';`) is
/// whatever that key is bound to, which the alias table in the
/// resolved-class cache knows: the framework's own
/// `registerCoreContainerAliases()` plus every key the project's service
/// providers bind.  A key bound only at runtime, or a cache with no alias
/// table, leaves the facade with whatever its `@method static` tags declare.
fn accessor_class(
    class: &ClassInfo,
    cache: Option<&ResolvedClassCache>,
) -> Option<Cow<'static, str>> {
    match class.laravel()?.facade_accessor? {
        FacadeAccessor::Class(fqn) => Some(Cow::Borrowed(fqn.as_str())),
        FacadeAccessor::Alias(key) => cache
            .and_then(|cache| cache.read().container_alias_concrete_fqn(key.as_str()))
            .map(Cow::Owned),
    }
}

/// The class a facade forwards its static calls to, or `None` when `class`
/// is not a facade or its accessor names nothing we can load.
pub(crate) fn facade_concrete_class(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Option<Arc<ClassInfo>> {
    if !LaravelFacadeProvider.applies_to(class, class_loader) {
        return None;
    }
    let concrete = class_loader(&accessor_class(class, cache)?)?;
    // A facade that names itself has nothing to forward, and resolving it
    // would re-enter the provider.
    (concrete.fqn() != class.fqn()).then_some(concrete)
}

/// Turn the concrete class's public instance methods into static virtual
/// methods on the facade.
fn build_forwarded_methods(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Vec<Arc<MethodInfo>> {
    let Some(concrete_fqn) = accessor_class(class, cache) else {
        return Vec::new();
    };
    let Some(concrete) = class_loader(&concrete_fqn) else {
        return Vec::new();
    };
    // A facade that names itself has nothing to forward, and resolving it
    // would re-enter this provider.
    if concrete.fqn() == class.fqn() {
        return Vec::new();
    }

    let resolved =
        crate::virtual_members::resolve_class_fully_maybe_cached(&concrete, class_loader, cache);

    // A fluent method returns the concrete instance, not the facade.
    let subs = super::self_ref_subs(PhpType::named(resolved.fqn()));
    let fp = crate::virtual_members::TransformFingerprint::new(
        Some(&subs),
        None,
        crate::virtual_members::cache::transform_flags::FORWARD_AS_STATIC,
    );

    let mut methods = Vec::new();
    for method in resolved.methods.iter() {
        // `__callStatic` forwards to an *instance*, so a static method on
        // the concrete class is not reachable through the facade, and a
        // magic method is an implementation detail of the forwarding.
        if method.is_static || method.visibility != Visibility::Public {
            continue;
        }
        if method.name.starts_with("__") {
            continue;
        }
        // The facade's own declarations and its `@method static` tags
        // (merged by the providers that ran before this one) win.
        if class
            .methods
            .iter()
            .any(|m| m.is_static && m.name.eq_ignore_ascii_case(&method.name))
        {
            continue;
        }

        methods.push(crate::virtual_members::intern_transformed_method(
            method,
            fp,
            || {
                let mut forwarded = (**method).clone();
                forwarded.is_static = true;
                forwarded.is_virtual = true;
                if let Some(ref mut ret) = forwarded.return_type {
                    *ret = ret.substitute(&subs);
                }
                forwarded
            },
        ));
    }
    methods
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "facade_tests.rs"]
mod tests;
