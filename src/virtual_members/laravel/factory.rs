//! Laravel Eloquent Factory virtual member provider.
//!
//! Synthesizes `create()` and `make()` methods for factory classes that
//! extend `Illuminate\Database\Eloquent\Factories\Factory` but do not
//! already have `@extends Factory<Model>` generics (which resolve those two
//! methods on their own). The model type comes from an explicit generic
//! binding, the factory's `$model` property, or Laravel's naming convention
//! (e.g. `Database\Factories\UserFactory` → `App\Models\User`), in that
//! order.
//!
//! In addition, it synthesizes the dynamic relationship methods that
//! Laravel's `Factory::__call()` resolves at runtime — `has{Relationship}()`
//! and `for{Relationship}()` for each relationship method on the associated
//! model, plus `trashed()` when the model uses `SoftDeletes`.  These return
//! `static` so the fluent chain stays on the factory (e.g.
//! `UserFactory::new()->hasPosts(3)->create()`), and are synthesized
//! regardless of whether `@extends Factory<Model>` generics are present,
//! since the generics system does not cover them.

use crate::atom::atom;
use crate::inheritance::{ClassRef, build_substitution_map};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, MethodInfo, ParameterInfo};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::helpers::{snake_to_pascal, walks_parent_chain};
use super::{classify_relationship_typed, is_soft_deletes_trait};

use super::super::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};

/// Check whether a trait name is the Laravel `HasFactory` trait.
///
/// Matches the FQN `Illuminate\Database\Eloquent\Factories\HasFactory`
/// as well as the short name `HasFactory` (common in same-file tests).
pub(crate) fn is_has_factory_trait(trait_name: &str) -> bool {
    trait_name == "Illuminate\\Database\\Eloquent\\Factories\\HasFactory"
        || trait_name == "HasFactory"
}

/// Check whether a parent class name is the Laravel
/// `Illuminate\Database\Eloquent\Factories\Factory` base class.
pub(crate) fn is_factory_class(class_name: &str) -> bool {
    class_name == "Illuminate\\Database\\Eloquent\\Factories\\Factory" || class_name == "Factory"
}

/// The fully-qualified name of the `Factory` base class.
const FACTORY_FQN: &str = "Illuminate\\Database\\Eloquent\\Factories\\Factory";

/// Derive the conventional factory FQN from a model FQN.
///
/// Follows Laravel's default convention:
/// - `App\Models\User` → `Database\Factories\UserFactory`
/// - `App\Models\Admin\SuperUser` → `Database\Factories\Admin\SuperUserFactory`
///
/// The rule: strip the `Models\` segment from the namespace, replace
/// the root with `Database\Factories\`, and append `Factory` to the
/// class short name.
pub(crate) fn model_to_factory_fqn(model_fqn: &str) -> String {
    let (ns, short) = match model_fqn.rsplit_once('\\') {
        Some((ns, short)) => (ns, short),
        None => return format!("Database\\Factories\\{model_fqn}Factory"),
    };

    // Check for `X\Models\Sub` pattern → `Database\Factories\Sub`
    if let Some((_prefix, suffix)) = ns.split_once("\\Models\\") {
        return format!("Database\\Factories\\{suffix}\\{short}Factory");
    }

    // Check for `X\Models` pattern (model directly in Models namespace)
    if ns.ends_with("\\Models") || ns == "Models" {
        return format!("Database\\Factories\\{short}Factory");
    }

    // No `Models` segment — put factory in `Database\Factories`
    format!("Database\\Factories\\{short}Factory")
}

/// Derive the conventional model FQN from a factory FQN.
///
/// Reverse of [`model_to_factory_fqn`]:
/// - `Database\Factories\UserFactory` → `App\Models\User`
/// - `Database\Factories\Admin\SuperUserFactory` → `App\Models\Admin\SuperUser`
pub(crate) fn factory_to_model_fqn(factory_fqn: &str) -> Option<String> {
    // The short name must end with `Factory`.
    let short = factory_fqn.rsplit('\\').next().unwrap_or(factory_fqn);
    let model_short = short.strip_suffix("Factory")?;
    if model_short.is_empty() {
        return None;
    }

    let ns = factory_fqn
        .rsplit_once('\\')
        .map(|(ns, _)| ns)
        .unwrap_or("");

    let sub_ns = if let Some(after) = ns.strip_prefix("Database\\Factories\\") {
        Some(after)
    } else if ns == "Database\\Factories" {
        None
    } else {
        // Not in the standard factory namespace, so there is no
        // sub-namespace to carry over.
        None
    };

    match sub_ns {
        Some(sub) => Some(format!("App\\Models\\{sub}\\{model_short}")),
        None => Some(format!("App\\Models\\{model_short}")),
    }
}

/// Determine whether `class_name` is the Eloquent Factory base class.
fn is_eloquent_factory(class_name: &str) -> bool {
    class_name == FACTORY_FQN
}

/// Walk the parent chain of `class` looking for
/// `Illuminate\Database\Eloquent\Factories\Factory`.
///
/// Returns `true` if the class itself is `Factory` or any ancestor is.
pub(crate) fn extends_eloquent_factory(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    walks_parent_chain(class, class_loader, is_eloquent_factory)
}

/// Check whether a factory class already has `@extends Factory<Model>`
/// that would let the generics system resolve `TModel`.
fn has_factory_extends_generic(class: &ClassInfo) -> bool {
    class.extends_generics.iter().any(|(name, args)| {
        let short = name.rsplit('\\').next().unwrap_or(name);
        short == "Factory" && !args.is_empty()
    })
}

/// Find a model declared explicitly by a factory hierarchy.
///
/// An `@extends Factory<Model>` binding may be forwarded through one or more
/// generic base factories. The nearest `$model` declaration is retained as a
/// fallback while that chain is walked, because an explicit generic binding
/// remains the strongest source when both are present. A template parameter
/// only becomes explicit evidence after the hierarchy binds it to a concrete
/// type.
fn declared_factory_model_type(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    // Preserve the allocation-free direct path, and allow isolated test/stub
    // classes to carry a complete declaration without loading the parent. A
    // raw template is not a model declaration yet: `$model` or the convention
    // must remain available until a concrete binding substitutes it.
    if let Some(model) = class.extends_generics.iter().find_map(|(name, args)| {
        let short = name.rsplit('\\').next().unwrap_or(name);
        (short == "Factory").then(|| args.last()).flatten()
    }) && !model.references_any_name(&class.template_params)
    {
        return Some(model.clone());
    }

    let mut template_params = class.template_params.clone();
    let mut current = ClassRef::Borrowed(class);
    let mut active_subs: HashMap<String, PhpType> = HashMap::new();
    let mut property_model = None;

    for _ in 0..MAX_INHERITANCE_DEPTH {
        if property_model.is_none() {
            property_model = current
                .laravel()
                .and_then(|metadata| metadata.factory_model.clone());
        }

        let Some(parent_name) = current.parent_class else {
            return property_model;
        };

        // The Laravel boundary itself need not be loaded. Its factory model
        // is the trailing `@extends Factory<…>` argument, after applying any
        // binding carried through the intermediate generic bases.
        if is_factory_class(&parent_name) {
            let generic_model = current
                .extends_generics
                .iter()
                .find_map(|(name, args)| {
                    let short = name.rsplit('\\').next().unwrap_or(name);
                    (short == "Factory").then(|| args.last()).flatten()
                })
                .map(|model| {
                    if active_subs.is_empty() {
                        model.clone()
                    } else {
                        model.substitute(&active_subs)
                    }
                })
                .filter(|model| !model.references_any_name(&template_params));
            return generic_model.or(property_model);
        }

        let Some(parent) = class_loader(&parent_name) else {
            return property_model;
        };
        let level_subs = build_substitution_map(&current, &parent, &active_subs);

        active_subs = level_subs;
        current = ClassRef::Owned(parent);
        template_params.extend_from_slice(&current.template_params);
    }

    property_model
}

/// Find the first loadable model implied by a factory name, from leaf to base.
fn conventional_factory_model_type(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let mut current = ClassRef::Borrowed(class);

    for _ in 0..MAX_INHERITANCE_DEPTH {
        if let Some(model_fqn) = factory_to_model_fqn(&current.fqn())
            && class_loader(&model_fqn).is_some()
        {
            return Some(PhpType::named(atom(&model_fqn)));
        }

        let parent_name = current.parent_class?;
        if is_factory_class(&parent_name) {
            return None;
        }
        current = ClassRef::Owned(class_loader(&parent_name)?);
    }

    None
}

/// The model type a factory class builds.
///
/// Prefers an explicit generic binding, including one forwarded through a
/// shared factory base, then the factory's declared `$model`, and finally the
/// naming convention (`Database\Factories\UserFactory` → `App\Models\User`).
/// The convention branch only answers when the model class actually exists,
/// so a `Factory` subclass that merely has a factory-shaped name does not
/// invent a model.
pub(crate) fn factory_model_type(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    declared_factory_model_type(class, class_loader)
        .or_else(|| conventional_factory_model_type(class, class_loader))
}

/// Build virtual `create()` and `make()` methods for a factory class
/// that does not have `@extends Factory<Model>`.
///
/// The model type is resolved once by the provider and shared with every
/// factory-specific member builder.
fn build_factory_model_methods(model_type: Option<&PhpType>) -> Vec<MethodInfo> {
    let model_type = match model_type {
        Some(ty) => ty,
        None => return Vec::new(),
    };

    vec![
        MethodInfo::virtual_method_typed("create", Some(model_type)),
        MethodInfo::virtual_method_typed("make", Some(model_type)),
    ]
}

/// Build an optional (non-required) parameter for a synthesized factory
/// relationship method.
fn relationship_param(name: &str, type_str: &str) -> ParameterInfo {
    ParameterInfo {
        name: atom(name),
        is_required: false,
        type_hint: Some(PhpType::parse(type_str)),
        native_type_hint: None,
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }
}

/// Whether `class` uses Laravel's `SoftDeletes` trait directly.
fn class_uses_soft_deletes_trait(class: &ClassInfo) -> bool {
    class.used_traits.iter().any(|t| is_soft_deletes_trait(t))
}

/// Whether `class` or any ancestor uses the `SoftDeletes` trait.
///
/// Full resolution keeps only the leaf class's `used_traits`, so callers
/// pass the *raw* model here and this walks the parent chain explicitly.
fn model_uses_soft_deletes(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if class_uses_soft_deletes_trait(class) {
        return true;
    }

    let mut current = class.parent_class;
    let mut depth = 0u32;
    while let Some(parent_name) = current {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            break;
        }
        match class_loader(&parent_name) {
            Some(parent) => {
                if class_uses_soft_deletes_trait(&parent) {
                    return true;
                }
                current = parent.parent_class;
            }
            None => break,
        }
    }

    false
}

/// Build virtual `has{Relationship}()`, `for{Relationship}()`, and (when
/// the model uses `SoftDeletes`) `trashed()` methods for a factory class.
///
/// Laravel's `Factory::__call()` resolves `hasPosts(...)` / `forAuthor(...)`
/// dynamically: it strips the `has`/`for` prefix, camelCases the
/// remainder, and delegates when that name is a valid relationship method
/// on the associated model.  Each synthesized method returns `static` so
/// the fluent chain stays on the factory.
///
/// The associated model is fully resolved so that relationships declared on
/// traits or parent classes are visible.
fn build_factory_relationship_methods(
    model_type: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Vec<MethodInfo> {
    let model_type = match model_type {
        Some(model) => model,
        None => return Vec::new(),
    };
    let Some(model_fqn) = model_type.base_name() else {
        return Vec::new();
    };

    let model = match class_loader(model_fqn) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let resolved =
        crate::virtual_members::resolve_class_fully_maybe_cached(&model, class_loader, cache);

    let factory_type = PhpType::static_();

    let mut methods = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for method in &resolved.methods {
        let Some(return_type) = method.return_type.as_ref() else {
            continue;
        };
        if classify_relationship_typed(return_type).is_none() {
            continue;
        }

        let pascal = snake_to_pascal(&method.name);
        if pascal.is_empty() {
            continue;
        }

        // has{Relationship}(int|array|callable $count, array|callable $state): static
        let has_name = format!("has{pascal}");
        if seen.insert(has_name.to_ascii_lowercase()) {
            methods.push(MethodInfo {
                parameters: vec![
                    relationship_param("$count", "int|array|callable"),
                    relationship_param("$state", "array|callable"),
                ]
                .into(),
                description: Some(format!(
                    "Create the `{}` relationship for the factory-created model(s).",
                    method.name
                )),
                ..MethodInfo::virtual_method_typed(&has_name, Some(&factory_type))
            });
        }

        // for{Relationship}(array|callable $state): static
        let for_name = format!("for{pascal}");
        if seen.insert(for_name.to_ascii_lowercase()) {
            methods.push(MethodInfo {
                parameters: vec![relationship_param("$state", "array|callable")].into(),
                description: Some(format!(
                    "Attach the factory-created model(s) to a `{}` parent.",
                    method.name
                )),
                ..MethodInfo::virtual_method_typed(&for_name, Some(&factory_type))
            });
        }
    }

    // trashed(): static — only when the model uses SoftDeletes.  Use the
    // raw model (not the flattened resolution) so the parent chain and
    // its `used_traits` remain walkable.
    if model_uses_soft_deletes(&model, class_loader) && seen.insert("trashed".to_string()) {
        methods.push(MethodInfo {
            description: Some(
                "Indicate that the factory-created model should be soft deleted.".to_string(),
            ),
            ..MethodInfo::virtual_method_typed("trashed", Some(&factory_type))
        });
    }

    methods
}

/// Virtual member provider for Laravel Eloquent factories.
///
/// When a class extends `Illuminate\Database\Eloquent\Factories\Factory`
/// (directly or through an intermediate parent) and does not already
/// have `@extends Factory<Model>` generics, this provider synthesizes
/// `create()` and `make()` methods that return the associated model type.
pub struct LaravelFactoryProvider;

impl VirtualMemberProvider for LaravelFactoryProvider {
    /// Returns `true` if the class extends
    /// `Illuminate\Database\Eloquent\Factories\Factory`.
    ///
    /// Applies even when the class already has `@extends Factory<Model>`
    /// generics: the generics system resolves `create()`/`make()` in that
    /// case, but not the dynamic relationship methods `provide()` still
    /// needs to synthesize.
    fn applies_to(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        !is_eloquent_factory(&class.name) && extends_eloquent_factory(class, class_loader)
    }

    /// Synthesize the dynamic `has{Relationship}()` / `for{Relationship}()`
    /// / `trashed()` methods resolved by Laravel's `Factory::__call()`, plus
    /// `create()` and `make()` when the class has no `@extends Factory<Model>`
    /// generics for the generics system to resolve them from instead.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&crate::virtual_members::ResolvedClassCache>,
    ) -> VirtualMembers {
        let model_type = factory_model_type(class, class_loader);
        let mut methods = if has_factory_extends_generic(class) {
            Vec::new()
        } else {
            build_factory_model_methods(model_type.as_ref())
        };
        methods.extend(build_factory_relationship_methods(
            model_type.as_ref(),
            class_loader,
            cache,
        ));
        VirtualMembers {
            methods: methods.into_iter().map(Arc::new).collect(),
            properties: Vec::new(),
            constants: Vec::new(),
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "factory_tests.rs"]
mod tests;
