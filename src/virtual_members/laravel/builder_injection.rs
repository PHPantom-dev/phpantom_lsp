//! Injection of model scopes and `@method` virtual methods onto a resolved
//! Eloquent Builder.
//!
//! Laravel's `Builder::__call()` forwards unknown method calls to the model,
//! and `Model::__callStatic()` forwards static calls to a Builder.  When the
//! type engine resolves a `Builder<ConcreteModel>` (either directly or through
//! an inherited `@mixin Builder<X>` on a relation), these helpers graft the
//! model's scope methods, `@method` tags, and `where{Column}()` methods onto
//! the builder so a chain like `User::where(...)->active()->withTrashed()`
//! resolves end-to-end.
//!
//! Called from `completion/types/resolution.rs` after a class has been
//! resolved and generic substitution applied.  Keeping the framework logic
//! here rather than inline in the generic resolver avoids coupling the type
//! engine to Laravel conventions.

use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::ClassInfo;
use crate::virtual_members::{ResolvedClassCache, resolve_class_fully_maybe_cached};

use super::helpers::{extends_eloquent_builder, extends_eloquent_model};
use super::where_property::{build_where_property_methods_for_class, lowercase_method_names};
use super::{ELOQUENT_BUILDER_FQN, build_scope_methods_for_builder, self_ref_subs};

/// Inject scope methods and model virtual methods onto a resolved Builder.
///
/// When the resolved class is the Eloquent Builder and the first generic
/// argument is a concrete model name, injects:
///
/// 1. **Scope methods** — `scopeX` and `#[Scope]` methods from the model,
///    with the `scope` prefix stripped and the first `$query` parameter
///    removed.
///
/// 2. **Model `@method` tags** — virtual methods declared via `@method`
///    on the model or its traits (e.g. `SoftDeletes`'s `withTrashed`).
///    Laravel's `Builder::__call` forwards unknown calls to the model,
///    so these methods are effectively available on the Builder instance.
///    Return types containing `static` are remapped to
///    `Builder<ConcreteModel>` to keep the chain on the builder.
///
/// The `cls` parameter is the Builder **after** generic substitution has
/// been applied.  `raw_cls` is the pre-substitution class (needed to
/// check the FQN via `file_namespace`).
pub(crate) fn try_inject_builder_scopes(
    result: &mut ClassInfo,
    raw_cls: &ClassInfo,
    base_fqn: &str,
    generic_args: &[PhpType],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    if !is_eloquent_builder_fqn(base_fqn, raw_cls, class_loader) || generic_args.is_empty() {
        return;
    }

    // The first (or only) generic arg is the model type.
    let model_name = match generic_args.first().unwrap().base_name() {
        Some(name) => name,
        None => return,
    };

    inject_scopes_and_model_methods(result, model_name, class_loader, None);
}

/// Inject scope methods and model virtual methods onto a class that has
/// a `@mixin Builder<TRelatedModel>` inherited from an ancestor.
///
/// When a class like `HasMany<ProductTranslation>` inherits
/// `@mixin Builder<TRelatedModel>` from grandparent `Relation`, the
/// mixin expansion adds Builder's own methods but does NOT inject
/// model-specific scopes.  Scopes are normally injected by
/// [`try_inject_builder_scopes`] which only fires when the resolved
/// class IS the Builder.
///
/// This function handles the inherited-mixin case: it walks the raw
/// class's parent chain, finds `@mixin Builder<X>` declarations,
/// applies the generic substitution map (built from the concrete
/// type arguments at the call site) to resolve `X` to a concrete
/// model name, and injects that model's scopes and `@method` virtual
/// methods.
pub(crate) fn try_inject_mixin_builder_scopes(
    result: &mut ClassInfo,
    raw_cls: &ClassInfo,
    generic_args: &[PhpType],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    use std::collections::HashMap;

    use crate::types::MAX_INHERITANCE_DEPTH;
    use crate::util::short_name;

    if generic_args.is_empty() || raw_cls.template_params.is_empty() {
        return;
    }

    // Build the substitution map from the class's own template params
    // to the concrete generic args provided at the call site.
    // e.g. for HasMany<ProductTranslation, Product>:
    //   TRelatedModel → ProductTranslation, TDeclaringModel → Product
    let mut root_subs: HashMap<String, PhpType> = HashMap::new();
    for (i, param_name) in raw_cls.template_params.iter().enumerate() {
        if let Some(arg) = generic_args.get(i) {
            root_subs.insert(param_name.to_string(), arg.clone());
        }
    }

    // Walk the parent chain looking for @mixin Builder<X> declarations.
    // At each level, build a substitution map that maps the parent's
    // template params to concrete types (threading through @extends
    // generics), then check if the parent has a Builder mixin.
    //
    // We use `ClassRef` to avoid lifetime issues when alternating
    // between a borrowed initial class and owned parent classes.
    let mut current = crate::inheritance::ClassRef::Borrowed(raw_cls);
    let mut active_subs = root_subs;
    let mut depth = 0u32;

    // Also check the class itself (it might directly declare @mixin Builder<X>).
    loop {
        // Check for Builder mixin on the current class.
        if let Some(model_name) =
            find_builder_mixin_model(&current, &active_subs, raw_cls, class_loader)
        {
            inject_scopes_and_model_methods(result, &model_name, class_loader, None);
            return;
        }

        // Move to the parent class.
        let parent_name = match current.parent_class {
            Some(name) => name,
            None => break,
        };
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            break;
        }
        let parent = match class_loader(&parent_name) {
            Some(p) => p,
            None => break,
        };

        // Build the substitution map for this level by combining the
        // child's @extends generics with the active substitutions.
        let parent_short = short_name(&parent.name);
        let type_args = current
            .extends_generics
            .iter()
            .find(|(name, _)| short_name(name) == parent_short)
            .map(|(_, args)| args);

        if let Some(args) = type_args {
            let mut level_subs = HashMap::new();
            for (i, param_name) in parent.template_params.iter().enumerate() {
                if let Some(arg) = args.get(i) {
                    let resolved = arg.substitute(&active_subs);
                    level_subs.insert(param_name.to_string(), resolved);
                }
            }
            active_subs = level_subs;
        }
        // If no @extends generics matched, the parent's template params
        // are unbound and we can't resolve the mixin's model type, so
        // we keep the current active_subs (they won't match parent
        // template param names, which is correct — the substitution
        // will be a no-op).

        current = crate::inheritance::ClassRef::Owned(parent);
    }
}

/// Check if a class declares `@mixin Builder<X>` and return the concrete
/// model name after applying substitutions.
///
/// Returns `Some(model_name)` when `X` resolves to a concrete type (not
/// a template parameter of the root class).  Returns `None` otherwise.
fn find_builder_mixin_model(
    class: &ClassInfo,
    active_subs: &std::collections::HashMap<String, crate::php_type::PhpType>,
    root_cls: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    use crate::util::short_name;

    for mixin_name in &class.mixins {
        if short_name(mixin_name) != "Builder" && mixin_name != ELOQUENT_BUILDER_FQN {
            continue;
        }
        // Verify it's actually the Eloquent Builder (not some other
        // class named Builder).  If we can't load it, trust the FQN.
        if let Some(ref mixin_cls) = class_loader(mixin_name) {
            let fqn = mixin_cls.fqn();
            if fqn != ELOQUENT_BUILDER_FQN && mixin_cls.name != ELOQUENT_BUILDER_FQN {
                continue;
            }
        }

        // Find the generic args for this mixin from mixin_generics.
        let mixin_short = short_name(mixin_name);
        let mixin_args = class
            .mixin_generics
            .iter()
            .find(|(name, _)| name == mixin_name || short_name(name) == mixin_short)
            .map(|(_, args)| args.as_slice());

        // Get the first generic arg (the model type) and substitute.
        if let Some(args) = mixin_args
            && let Some(first_arg) = args.first()
        {
            let resolved = first_arg.substitute(active_subs);
            if let Some(name) = resolved.base_name()
                && !root_cls.template_params.iter().any(|p| p == name)
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Shared helper: inject scope methods and `@method` virtual methods
/// from a model onto a class (Builder or a class with a Builder mixin).
fn inject_scopes_and_model_methods(
    result: &mut ClassInfo,
    model_arg: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) {
    // 1. Inject scope methods.
    let scope_methods = build_scope_methods_for_builder(model_arg, class_loader);
    for method in scope_methods {
        let already_exists = result
            .methods
            .iter()
            .any(|m| m.name == method.name && m.is_static == method.is_static);
        if !already_exists {
            result.methods.push(Arc::new(method));
        }
    }

    // 2. Inject @method virtual methods from the model.
    inject_model_virtual_methods(result, model_arg, class_loader, cache);

    // 3. Inject where{PropertyName}() dynamic methods from the model's
    //    known columns.  These are instance methods on the Builder so
    //    that `$query->whereBrandId(42)` resolves.
    if let Some(model_class) = class_loader(model_arg) {
        let existing = lowercase_method_names(&result.methods);
        let where_methods = build_where_property_methods_for_class(&model_class, &existing);
        for method in where_methods {
            if !result
                .methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(&method.name))
            {
                result.methods.push(Arc::new(method));
            }
        }
    }
}

/// Inject `@method`-declared virtual methods from a model onto a Builder.
///
/// Laravel's `Builder::__call()` forwards unknown method calls to the
/// model instance.  This means `@method` tags on the model (including
/// those inherited from traits like `SoftDeletes`) are callable on the
/// Builder.  For example:
///
/// ```php
/// // SoftDeletes declares: @method static Builder<static> withTrashed()
/// // Customer uses SoftDeletes
/// Customer::groupBy('email')->withTrashed()->first()
/// //                          ^^^^^^^^^^^^^ needs to resolve on Builder<Customer>
/// ```
///
/// This function loads the fully-resolved model, finds virtual methods
/// (those with `is_virtual = true`, which come from `@method` tags),
/// and injects them as **instance** methods on the Builder.  Return
/// types containing `static`, `self`, or `$this` are substituted with
/// `Builder<ConcreteModel>` so the chain continues on the builder.
fn inject_model_virtual_methods(
    builder: &mut ClassInfo,
    model_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) {
    let model_class = match class_loader(model_name) {
        Some(c) => c,
        None => return,
    };

    if !extends_eloquent_model(&model_class, class_loader) {
        return;
    }

    // Resolve the model fully so that @method tags from traits and
    // parent classes are included.
    let resolved_model = resolve_class_fully_maybe_cached(&model_class, class_loader, cache);

    // Build a substitution map: `static`/`self`/`$this` in return
    // types should become the concrete model name.  The `@method`
    // tags already declare the full return type (e.g.
    // `Builder<static>`), so substituting `static` → model name
    // produces `Builder<Customer>`.  Using `Builder<Model>` here
    // would double-wrap to `Builder<Builder<Customer>>`.
    let model_type = PhpType::Named(model_name.to_owned());
    let subs = self_ref_subs(model_type);

    for method in &resolved_model.methods {
        // Only inject virtual methods (from @method tags).  Real
        // methods on the model are not forwarded through Builder.
        if !method.is_virtual {
            continue;
        }

        // Skip methods already present on the builder (real methods,
        // scope methods, or previously injected methods).
        if builder
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(&method.name))
        {
            continue;
        }

        // Clone the inner MethodInfo and convert to an instance method on the builder.
        let mut forwarded = (**method).clone();
        forwarded.is_static = false;

        // Substitute self-referencing return types.
        if let Some(ref mut ret) = forwarded.return_type {
            *ret = ret.substitute(&subs);
        }

        builder.methods.push(Arc::new(forwarded));
    }
}

/// Check whether a base FQN and/or a `ClassInfo` refer to the Eloquent Builder.
///
/// Handles the three forms a Builder can appear as:
/// 1. The type hint FQN itself (e.g. from `@return Builder<User>`).
/// 2. The `ClassInfo.name` field (short name or FQN depending on source).
/// 3. The FQN constructed from `file_namespace + name` (PSR-4 loaded classes
///    where `name` is the short name only).
///
/// Also checks whether the class extends the base Eloquent Builder.
fn is_eloquent_builder_fqn(
    base_fqn: &str,
    cls: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    base_fqn == ELOQUENT_BUILDER_FQN
        || cls.name == ELOQUENT_BUILDER_FQN
        || cls.fqn() == ELOQUENT_BUILDER_FQN
        || extends_eloquent_builder(cls, class_loader)
}
