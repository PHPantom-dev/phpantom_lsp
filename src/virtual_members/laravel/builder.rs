//! Eloquent Builder-as-static forwarding.
//!
//! Laravel's `Model::__callStatic()` delegates static calls to
//! `static::query()`, which returns an Eloquent Builder.  This module
//! loads the Builder class, fully resolves it (including `@mixin`
//! `Query\Builder` members), and converts each public instance method
//! into a static virtual method on the model.
//!
//! Return type mapping:
//! - `static`, `$this`, `self` → `\Illuminate\Database\Eloquent\Builder<ConcreteModel>`
//!   (the chain continues on the builder, not the model).
//! - Template parameters (e.g. `TModel`) → the concrete model class name.
//!
//! Methods whose name starts with `__` (magic methods) are skipped.

use std::sync::Arc;

use crate::inheritance::{ancestors, apply_substitution_to_conditional};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo, Visibility};
use crate::virtual_members::ResolvedClassCache;

use super::ELOQUENT_BUILDER_FQN;

/// The custom Eloquent builder a model declares, or inherits from the
/// nearest parent that declares one.
///
/// `#[UseEloquentBuilder]` and `HasBuilder` sit on the class that names
/// the builder but apply to every subclass.
pub(crate) fn custom_builder_fqn(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let declared = |candidate: &ClassInfo| {
        candidate
            .laravel()
            .and_then(|l| l.custom_builder.as_ref())
            .and_then(|b| b.base_name())
            .map(str::to_string)
    };
    declared(class)
        .or_else(|| ancestors(class, class_loader).find_map(|(_, parent)| declared(&parent)))
}

/// Build static virtual methods by forwarding Eloquent Builder's public
/// instance methods onto the model class. See the module docs for the
/// return-type mapping.
pub(super) fn build_builder_forwarded_methods(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Vec<Arc<MethodInfo>> {
    let requested_builder_fqn =
        custom_builder_fqn(class, class_loader).unwrap_or_else(|| ELOQUENT_BUILDER_FQN.to_string());

    // Load the Eloquent Builder class (or custom builder).
    let (builder_class, builder_fqn) = match class_loader(&requested_builder_fqn) {
        Some(c) => (c, requested_builder_fqn),
        // Fallback to standard builder if custom builder fails to load.
        None if requested_builder_fqn != ELOQUENT_BUILDER_FQN => {
            match class_loader(ELOQUENT_BUILDER_FQN) {
                Some(c) => (c, ELOQUENT_BUILDER_FQN.to_string()),
                None => return Vec::new(),
            }
        }
        None => return Vec::new(),
    };

    // Fully resolve Builder (own + traits + parents + virtual members
    // including @mixin Query\Builder).  This is safe because Builder
    // does not extend Model, so the LaravelModelProvider will not
    // recurse.
    let resolved_builder = crate::virtual_members::resolve_class_fully_maybe_cached(
        &builder_class,
        class_loader,
        cache,
    );
    let effective_methods = builder_methods_with_unsubstituted_parent_templates(
        &builder_class,
        &resolved_builder,
        class_loader,
        cache,
    );

    // Build a substitution map: TModel → concrete model class name,
    // and static/$this/self → Builder<ConcreteModel>.
    let builder_self_type = PhpType::generic(&builder_fqn, vec![PhpType::named(class.fqn())]);
    let mut subs = super::self_ref_subs(builder_self_type.clone());
    insert_builder_template_substitutions(
        &mut subs,
        &builder_class,
        class,
        &builder_fqn,
        class_loader,
    );

    let mut methods = Vec::new();

    // The transform below is identical for every materialization of the
    // same model's forwarded set (the substitution map already encodes
    // the concrete model), so the copies are interned: repeated
    // resolutions and generic variants of the model share one
    // allocation per forwarded method.  The custom-collection rewrite
    // is part of the transform, so it participates in the fingerprint
    // through the extra string slot.
    let custom_collection = class
        .laravel()
        .and_then(|l| l.custom_collection.as_ref())
        .and_then(|c| c.base_name());
    let fp = crate::virtual_members::TransformFingerprint::new(
        Some(&subs),
        custom_collection,
        crate::virtual_members::cache::transform_flags::FORWARD_AS_STATIC,
    );

    for method in &effective_methods {
        if method.visibility != Visibility::Public {
            continue;
        }
        if method.name.starts_with("__") {
            continue;
        }
        // Skip methods already present on the model (real methods,
        // scope methods, etc.).  The merge logic in
        // `merge_virtual_members` would also skip them, but filtering
        // here avoids unnecessary cloning and substitution work.
        if class
            .methods
            .iter()
            .any(|m| m.name == method.name && m.is_static)
        {
            continue;
        }

        let forwarded = crate::virtual_members::intern_transformed_method(method, fp, || {
            let mut forwarded = (**method).clone();
            forwarded.is_static = true;

            // Apply template and self-type substitutions.
            if !subs.is_empty() {
                if let Some(ref mut ret) = forwarded.return_type {
                    *ret = ret.substitute(&subs);
                }
                if let Some(ref mut cond) = forwarded.conditional_return {
                    apply_substitution_to_conditional(cond, &subs);
                }
                for param in forwarded.parameters.make_mut() {
                    if let Some(ref mut hint) = param.type_hint {
                        *hint = hint.substitute(&subs);
                    }
                }
            }

            // Rewrite the base Eloquent Collection to whichever collection
            // the model in the (now substituted) signature builds.  A
            // `chunk()` callback is handed that collection just as `get()`
            // returns it.
            if let Some(ref mut ret) = forwarded.return_type
                && let Some(rewritten) =
                    super::replace_eloquent_collections_in_type(ret, class_loader)
            {
                *ret = rewritten;
            }
            if forwarded.parameters.iter().any(|p| {
                p.type_hint
                    .as_ref()
                    .is_some_and(super::mentions_eloquent_collection)
            }) {
                for param in forwarded.parameters.make_mut() {
                    if let Some(rewritten) = param.type_hint.as_ref().and_then(|hint| {
                        super::replace_eloquent_collections_in_param_type(hint, class_loader)
                    }) {
                        param.type_hint = Some(rewritten);
                    }
                }
            }

            forwarded
        });
        methods.push(forwarded);
    }

    // ── query() / newQuery() / newModelQuery() ──────────────────────
    // When a model has a custom builder, User::query() should return
    // UserBuilder<User> instead of the default Builder<User>.
    for name in ["query", "newQuery", "newModelQuery"] {
        methods.push(Arc::new(MethodInfo {
            is_static: true,
            ..MethodInfo::virtual_method_typed(name, Some(&builder_self_type))
        }));
    }

    methods
}

fn builder_methods_with_unsubstituted_parent_templates(
    builder_class: &ClassInfo,
    resolved_builder: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Vec<Arc<MethodInfo>> {
    let mut methods: Vec<Arc<MethodInfo>> = resolved_builder.methods.iter().cloned().collect();
    if builder_class.fqn() == ELOQUENT_BUILDER_FQN || builder_class.name == ELOQUENT_BUILDER_FQN {
        return methods;
    }

    let Some(base_builder) = class_loader(ELOQUENT_BUILDER_FQN) else {
        return methods;
    };
    let resolved_base = crate::virtual_members::resolve_class_fully_maybe_cached(
        &base_builder,
        class_loader,
        cache,
    );

    for parent_method in &resolved_base.methods {
        if custom_builder_chain_declares_method(builder_class, &parent_method.name, class_loader) {
            continue;
        }

        if let Some(existing) = methods
            .iter_mut()
            .find(|m| m.name.eq_ignore_ascii_case(&parent_method.name))
        {
            *existing = Arc::clone(parent_method);
        } else {
            methods.push(Arc::clone(parent_method));
        }
    }

    methods
}

fn custom_builder_chain_declares_method(
    builder_class: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    use crate::inheritance::ClassRef;

    // Walk the parent chain via `ClassRef` cursors so only the loader's
    // `Arc` is moved at each step (no per-step `ClassInfo` clone).
    let mut current = ClassRef::Borrowed(builder_class);
    // Cyclic inheritance (`class A extends B; class B extends A;`) is illegal
    // PHP but appears transiently while a file is being edited.  Track visited
    // FQNs and stop as soon as one repeats so the walk always terminates,
    // regardless of chain depth.
    let mut visited = crate::atom::AtomSet::default();
    loop {
        let fqn = current.fqn();
        if fqn == ELOQUENT_BUILDER_FQN || current.name == ELOQUENT_BUILDER_FQN {
            return false;
        }

        if current
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(method_name))
        {
            return true;
        }

        if !visited.insert(fqn) {
            return false;
        }

        let Some(parent) = current.parent_class.as_deref() else {
            return false;
        };
        if parent == ELOQUENT_BUILDER_FQN {
            return false;
        }

        let Some(parent_class) = class_loader(parent) else {
            return false;
        };
        current = ClassRef::Owned(parent_class);
    }
}

fn insert_builder_template_substitutions(
    subs: &mut std::collections::HashMap<String, PhpType>,
    builder_class: &ClassInfo,
    model_class: &ClassInfo,
    builder_fqn: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    let model_type = PhpType::named(model_class.fqn());
    for param in &builder_class.template_params {
        subs.insert(param.to_string(), model_type.clone());
    }

    if builder_fqn != ELOQUENT_BUILDER_FQN
        && let Some(base_builder) = class_loader(ELOQUENT_BUILDER_FQN)
    {
        for param in &base_builder.template_params {
            subs.entry(param.to_string())
                .or_insert_with(|| model_type.clone());
        }
    }

    subs.entry("TModel".to_string()).or_insert(model_type);
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
