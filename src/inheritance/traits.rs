//! Trait resolution: merging trait members into classes.
//!
//! This module handles `use Trait` statements, trait composition, alias/precedence
//! declarations, and generic trait substitution. It also includes Laravel-specific
//! convention-based factory trait detection.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::{Atom, atom};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MAX_TRAIT_DEPTH, Visibility};
use crate::util::short_name;
use crate::virtual_members::laravel::{extends_eloquent_model, model_to_factory_fqn};

use super::generics::{
    apply_substitution_to_method, apply_substitution_to_property, right_align_offset,
};
use super::{MergeDedup, TraitContext};

/// Merge all traits used by a class into its method/property/constant lists.
///
/// Walks the class's `used_traits`, recursively resolves trait composition,
/// applies alias/precedence declarations, and handles generic trait substitution
/// via `@use Trait<Type>` annotations.
///
/// **Precedence:** `insteadof` declarations exclude trait methods; `as` declarations
/// create aliases or change visibility. When a child class overrides a trait method,
/// the child's version is never merged (dedup check prevents it).
///
/// **Generics:** If the trait has `@template` parameters and the class declares
/// `@use Trait<Type>`, a substitution map is built and applied to all trait
/// members. Convention-based fallbacks (e.g., Laravel's `HasFactory` → derived
/// factory FQN) are attempted when no explicit `@use` generics are present.
pub(crate) fn merge_traits_into(
    merged: &mut ClassInfo,
    trait_names: &[Atom],
    ctx: &TraitContext<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u32,
    dedup: &mut MergeDedup,
    self_class_name: &str,
) {
    if depth > MAX_TRAIT_DEPTH {
        return;
    }

    for trait_name in trait_names {
        let trait_info = if let Some(t) = class_loader(trait_name) {
            t
        } else {
            continue;
        };

        // Build a substitution map for this trait if the using class
        // declared `@use TraitName<Type1, Type2>` and the trait has
        // `@template` parameters.
        let mut trait_subs =
            build_trait_substitution_map(trait_name, &trait_info, ctx.use_generics);

        // ── Convention-based HasFactory fallback ─────────────────
        // When a model uses `HasFactory` without `@use HasFactory<X>`,
        // derive the factory class from the naming convention
        // (e.g. `App\Models\User` → `Database\Factories\UserFactory`)
        // and substitute `TFactory` automatically.
        if trait_subs.is_empty()
            && !trait_info.template_params.is_empty()
            && is_has_factory_trait(trait_name)
            && extends_eloquent_model(merged, class_loader)
        {
            let model_fqn = merged.fqn();
            let factory_fqn = model_to_factory_fqn(&model_fqn);
            if class_loader(&factory_fqn).is_some() {
                for param in &trait_info.template_params {
                    trait_subs.insert(param.to_string(), PhpType::Named(factory_fqn.clone()));
                }
            }
        }

        // ── Template bound fallback ─────────────────────────────
        // When a class uses a generic trait without `@use` generics
        // and no convention-based provider filled the map, fall back
        // to the template parameter bounds (e.g. `@template T of object`
        // → `object`) so inherited methods don't leak raw template names.
        if !trait_info.template_params.is_empty() {
            for param_name in &trait_info.template_params {
                if !trait_subs.contains_key(param_name.to_string().as_str()) {
                    let bound = trait_info
                        .template_param_bounds
                        .get(param_name)
                        .cloned()
                        .unwrap_or_else(PhpType::mixed);
                    trait_subs.insert(param_name.to_string(), bound);
                }
            }
        }

        // Recursively merge traits used by this trait (trait composition).
        // The sub-trait's own `@use` generics (from the trait's docblock)
        // apply, not the outer class's.
        if !trait_info.used_traits.is_empty() {
            merge_traits_into(
                merged,
                &trait_info.used_traits,
                &TraitContext {
                    use_generics: &trait_info.use_generics,
                    precedences: &trait_info.trait_precedences,
                    aliases: &trait_info.trait_aliases,
                },
                class_loader,
                depth + 1,
                dedup,
                self_class_name,
            );
        }

        // Walk the `parent_class` (extends) chain so that interface
        // inheritance is resolved.  For example, `BackedEnum extends
        // UnitEnum` — loading `BackedEnum` alone would miss `UnitEnum`'s
        // members (`cases()`, `$name`) unless we follow the chain here.
        // The same depth counter is shared to prevent infinite loops.
        let mut current = trait_info.clone();
        let mut parent_depth = depth;
        while let Some(ref parent_name) = current.parent_class {
            parent_depth += 1;
            if parent_depth > MAX_TRAIT_DEPTH {
                break;
            }
            let parent = if let Some(p) = class_loader(parent_name) {
                p
            } else {
                break;
            };

            // Also follow the parent's own used_traits.
            if !parent.used_traits.is_empty() {
                merge_traits_into(
                    merged,
                    &parent.used_traits,
                    &TraitContext {
                        use_generics: &parent.use_generics,
                        precedences: &parent.trait_precedences,
                        aliases: &parent.trait_aliases,
                    },
                    class_loader,
                    parent_depth + 1,
                    dedup,
                    self_class_name,
                );
            }

            // Merge parent methods (skip private, skip duplicates)
            for method in &parent.methods {
                if method.visibility == Visibility::Private {
                    continue;
                }
                if !dedup
                    .methods
                    .insert(crate::atom::ascii_lowercase_atom(&method.name))
                {
                    continue;
                }
                merged.methods.push(Arc::clone(method));
            }

            // Merge parent properties
            for property in &parent.properties {
                if property.visibility == Visibility::Private {
                    continue;
                }
                if !dedup.properties.insert(property.name) {
                    continue;
                }
                merged.properties.push(property.clone());
            }

            // Merge parent constants
            for constant in &parent.constants {
                if constant.visibility == Visibility::Private {
                    continue;
                }
                if !dedup.constants.insert(constant.name) {
                    continue;
                }
                merged.constants.push(constant.clone());
            }

            current = parent;
        }

        // Merge trait methods — skip if already present.
        // Apply generic substitution if a `@use` mapping exists.
        // Also skip methods excluded by `insteadof` declarations.
        for method in &trait_info.methods {
            // Check if this method from this trait is excluded by an
            // `insteadof` declaration.  For example, if the class has
            // `TraitA::method insteadof TraitB`, then when merging
            // TraitB's methods, `method` should be skipped.
            let excluded = ctx.precedences.iter().any(|p| {
                p.method_name.eq_ignore_ascii_case(&method.name)
                    && p.insteadof
                        .iter()
                        .any(|excluded_trait| excluded_trait == trait_name)
            });
            if excluded {
                continue;
            }

            if !dedup
                .methods
                .insert(crate::atom::ascii_lowercase_atom(&method.name))
            {
                continue;
            }
            let mut method = (**method).clone();

            // Apply visibility-only `as` changes (no alias name).
            // For example, `TraitA::method as protected` changes the
            // visibility of `method` without creating an alias.
            for alias in ctx.aliases {
                if alias.method_name.eq_ignore_ascii_case(&method.name)
                    && alias.alias.is_none()
                    && let Some(vis) = alias.visibility
                {
                    // Check trait name matches (if specified)
                    let name_matches = alias.trait_name.as_ref().is_none_or(|t| t == trait_name);
                    if name_matches {
                        method.visibility = vis;
                    }
                }
            }

            if !trait_subs.is_empty() {
                apply_substitution_to_method(&mut method, &trait_subs);
            }
            // Replace bare `self` with the using class name so that
            // `self` resolves to the class that imports the trait.
            if let Some(ref mut rt) = method.return_type
                && rt.contains_bare_self()
            {
                *rt = rt.replace_bare_self(self_class_name);
            }
            merged.methods.push(Arc::new(method));
        }

        // Merge trait properties — apply substitution.
        for property in &trait_info.properties {
            if !dedup.properties.insert(property.name) {
                continue;
            }
            let mut property = property.clone();
            if !trait_subs.is_empty() {
                apply_substitution_to_property(&mut property, &trait_subs);
            }
            merged.properties.push(property);
        }

        // Merge trait constants
        for constant in &trait_info.constants {
            if !dedup.constants.insert(constant.name) {
                continue;
            }
            merged.constants.push(constant.clone());
        }

        // Apply `as` alias declarations that create new method names.
        // For example, `TraitB::method as traitBMethod` creates a copy
        // of `method` accessible as `traitBMethod`.
        for alias in ctx.aliases {
            // Only process aliases that have a new name.
            let alias_name = match &alias.alias {
                Some(name) => name,
                None => continue,
            };

            // Check trait name matches (if specified).
            let name_matches = alias.trait_name.as_ref().is_none_or(|t| t == trait_name);
            if !name_matches {
                continue;
            }

            // Find the source method in this trait.
            let source_method = trait_info
                .methods
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(&alias.method_name));
            let source_method = match source_method {
                Some(m) => m,
                None => continue,
            };

            // Skip if an alias with this name already exists.
            let alias_atom = atom(alias_name);
            if !dedup
                .methods
                .insert(crate::atom::ascii_lowercase_atom(alias_name))
            {
                continue;
            }

            let mut aliased = (**source_method).clone();
            aliased.name = alias_atom;
            if let Some(vis) = alias.visibility {
                aliased.visibility = vis;
            }
            if !trait_subs.is_empty() {
                apply_substitution_to_method(&mut aliased, &trait_subs);
            }
            merged.methods.push(Arc::new(aliased));
        }
    }
}

/// Check whether a trait name is the Laravel `HasFactory` trait.
///
/// Matches the FQN `Illuminate\Database\Eloquent\Factories\HasFactory`
/// as well as the short name `HasFactory` (common in same-file tests).
fn is_has_factory_trait(trait_name: &str) -> bool {
    trait_name == "Illuminate\\Database\\Eloquent\\Factories\\HasFactory"
        || trait_name == "HasFactory"
}

/// Check whether a parent class name is the Laravel
/// `Illuminate\Database\Eloquent\Factories\Factory` base class.
pub(crate) fn is_factory_class(class_name: &str) -> bool {
    class_name == "Illuminate\\Database\\Eloquent\\Factories\\Factory" || class_name == "Factory"
}

/// Build a substitution map for a trait based on `@use` generics and the
/// trait's `@template` parameters.
///
/// If the using class declares `@use HasFactory<UserFactory>` and the
/// trait `HasFactory` has `@template TFactory`, the returned map is
/// `{TFactory => UserFactory}`.
fn build_trait_substitution_map(
    trait_name: &str,
    trait_info: &ClassInfo,
    use_generics: &[(Atom, Vec<PhpType>)],
) -> HashMap<String, PhpType> {
    if trait_info.template_params.is_empty() || use_generics.is_empty() {
        return HashMap::new();
    }

    let trait_short = short_name(trait_name);

    // Find the @use entry that matches this trait.
    let type_args = use_generics
        .iter()
        .find(|(name, _)| {
            let name_short = short_name(name);
            name_short == trait_short
        })
        .map(|(_, args)| args);

    let type_args = match type_args {
        Some(args) => args,
        None => return HashMap::new(),
    };

    let mut map = HashMap::new();
    // Right-align a short argument list to the trailing template params,
    // matching PHPStan/Psalm convention for `@use Collection<User>`.
    let offset = right_align_offset(
        &trait_info.template_params,
        &trait_info.template_param_bounds,
        type_args.len(),
    );
    for (i, param_name) in trait_info.template_params.iter().enumerate() {
        if i < offset {
            let fallback = trait_info
                .template_param_bounds
                .get(param_name)
                .cloned()
                .unwrap_or_else(PhpType::mixed);
            map.insert(param_name.to_string(), fallback);
            continue;
        }
        if let Some(arg) = type_args.get(i - offset) {
            map.insert(param_name.to_string(), arg.clone());
        }
    }
    map
}
