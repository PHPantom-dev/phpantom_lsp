//! Laravel Eloquent Factory virtual member provider.
//!
//! Synthesizes `create()` and `make()` methods for factory classes that
//! extend `Illuminate\Database\Eloquent\Factories\Factory` but do not
//! already have `@extends Factory<Model>` generics.  The model type is
//! derived from the naming convention (e.g.
//! `Database\Factories\UserFactory` → `App\Models\User`).
//!
//! In addition, it synthesizes the dynamic relationship methods that
//! Laravel's `Factory::__call()` resolves at runtime — `has{Relationship}()`
//! and `for{Relationship}()` for each relationship method on the associated
//! model, plus `trashed()` when the model uses `SoftDeletes`.  These return
//! `static` so the fluent chain stays on the factory (e.g.
//! `UserFactory::new()->hasPosts(3)->create()`).

use crate::atom::atom;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, MethodInfo, ParameterInfo};
use std::collections::HashSet;
use std::sync::Arc;

use super::classify_relationship_typed;
use super::helpers::{snake_to_pascal, walks_parent_chain};

use super::super::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};

/// The fully-qualified name of the `Factory` base class.
const FACTORY_FQN: &str = "Illuminate\\Database\\Eloquent\\Factories\\Factory";

/// The fully-qualified name of Laravel's `SoftDeletes` trait.
const SOFT_DELETES_FQN: &str = "Illuminate\\Database\\Eloquent\\SoftDeletes";

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
    // Split into namespace + short name.
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

    // Extract the namespace after `Database\Factories\`.
    let ns = factory_fqn
        .rsplit_once('\\')
        .map(|(ns, _)| ns)
        .unwrap_or("");

    let sub_ns = if let Some(after) = ns.strip_prefix("Database\\Factories\\") {
        Some(after)
    } else if ns == "Database\\Factories" {
        None
    } else {
        // Not in the standard factory namespace — still try to strip
        // any `Factories` segment.
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
fn extends_eloquent_factory(
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

/// Build virtual `create()` and `make()` methods for a factory class
/// that does not have `@extends Factory<Model>`.
///
/// The model type is derived from the naming convention (e.g.
/// `Database\Factories\UserFactory` → `App\Models\User`).
fn build_factory_model_methods(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<MethodInfo> {
    let model_fqn = match factory_to_model_fqn(&class.name) {
        Some(fqn) => fqn,
        None => return Vec::new(),
    };

    // Verify the model class actually exists.
    if class_loader(&model_fqn).is_none() {
        return Vec::new();
    }

    let model_type = PhpType::Named(model_fqn.to_string());

    vec![
        MethodInfo::virtual_method_typed("create", Some(&model_type)),
        MethodInfo::virtual_method_typed("make", Some(&model_type)),
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
///
/// `used_traits` may hold the FQN or the imported short name, so we match
/// all three forms — mirroring the established `class_uses_conditionable`
/// detector.
fn class_uses_soft_deletes_trait(class: &ClassInfo) -> bool {
    class
        .used_traits
        .iter()
        .any(|t| t == SOFT_DELETES_FQN || t == "SoftDeletes" || t.ends_with("\\SoftDeletes"))
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
/// The model is loaded via the naming convention and fully resolved so
/// that relationships declared on traits or parent classes are visible.
fn build_factory_relationship_methods(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Vec<MethodInfo> {
    let model_fqn = match factory_to_model_fqn(&class.name) {
        Some(fqn) => fqn,
        None => return Vec::new(),
    };

    let model = match class_loader(&model_fqn) {
        Some(m) => m,
        None => return Vec::new(),
    };

    // Fully resolve the model so relationships declared on traits or
    // parent classes are visible.
    let resolved =
        crate::virtual_members::resolve_class_fully_maybe_cached(&model, class_loader, cache);

    // Factory relationship methods return the factory itself so the
    // fluent chain continues (e.g. `->hasPosts(3)->create()`).
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
                ],
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
                parameters: vec![relationship_param("$state", "array|callable")],
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
/// `create()` and `make()` methods that return the model type derived
/// from the naming convention.
pub struct LaravelFactoryProvider;

impl VirtualMemberProvider for LaravelFactoryProvider {
    /// Returns `true` if the class extends
    /// `Illuminate\Database\Eloquent\Factories\Factory` and does not
    /// already have `@extends Factory<Model>` generics.
    fn applies_to(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        !is_eloquent_factory(&class.name)
            && !has_factory_extends_generic(class)
            && extends_eloquent_factory(class, class_loader)
    }

    /// Synthesize `create()` and `make()` methods that return the model
    /// type derived from the naming convention, plus the dynamic
    /// `has{Relationship}()` / `for{Relationship}()` / `trashed()` methods
    /// resolved by Laravel's `Factory::__call()`.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&crate::virtual_members::ResolvedClassCache>,
    ) -> VirtualMembers {
        let mut methods = build_factory_model_methods(class, class_loader);
        methods.extend(build_factory_relationship_methods(class, class_loader, cache));
        VirtualMembers {
            methods,
            properties: Vec::new(),
            constants: Vec::new(),
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "factory_tests.rs"]
mod tests;
