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

use crate::atom::atom;
use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo};
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
/// The `result` parameter is the Builder **after** generic substitution has
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

    inject_scopes_and_model_methods(result, model_name, class_loader, None, None);
}

/// The type an injected Builder-typed return becomes when the methods are
/// grafted onto a class that forwards through
/// `ForwardsCalls::forwardDecoratedCallTo` (an Eloquent relation).
///
/// At runtime `Relation::__call` delegates to the query builder and returns
/// `$this` (the relation) whenever the forwarded call returned the builder,
/// so a chain like `$this->belongsTo(Author::class)->withTrashed()` stays on
/// the relation.  Any return the Builder is a supertype of is therefore
/// rewritten to the relation, which is what the Laravel PHPStan extensions
/// do as well.
///
/// The concrete relation type is stored rather than the `$this` keyword
/// because a self-like return only resolves through a receiver that carries
/// the method itself, and these methods live on the instantiated relation
/// alone (see the self-like fast path in
/// `type_engine::call_resolution::return_types`).
struct ForwardedSelf {
    ty: PhpType,
    /// `ty` in display form, to key the interning fingerprint by.
    fp_key: String,
}

impl ForwardedSelf {
    fn new(ty: PhpType) -> Self {
        let fp_key = ty.to_string();
        Self { ty, fp_key }
    }
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
        if let Some(model_name) =
            find_builder_mixin_model(&current, &active_subs, raw_cls, class_loader)
        {
            // A Relation forwards through `ForwardsCalls`, so a call that
            // returns the builder returns the relation instead.  The
            // injected methods carry Builder-typed returns (they are
            // written for the Builder), so they are rewritten to the
            // relation type as they are grafted on.
            let forwarded =
                crate::virtual_members::phpdoc::uses_forwards_calls(raw_cls, class_loader).then(
                    || ForwardedSelf::new(PhpType::generic(result.fqn(), generic_args.to_vec())),
                );
            inject_scopes_and_model_methods(
                result,
                &model_name,
                class_loader,
                None,
                forwarded.as_ref(),
            );
            return;
        }

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
    forwarded: Option<&ForwardedSelf>,
) {
    // 1. Inject scope methods.
    let scope_methods = build_scope_methods_for_builder(model_arg, class_loader);
    for method in scope_methods {
        let already_exists = result
            .methods
            .iter()
            .any(|m| m.name == method.name && m.is_static == method.is_static);
        if !already_exists {
            result
                .methods
                .push(forward_builder_return(&method, forwarded, class_loader));
        }
    }

    // 2. Inject @method virtual methods from the model.
    inject_model_virtual_methods(result, model_arg, class_loader, cache, forwarded);

    // 3. Inject where{PropertyName}() dynamic methods from the model's
    //    known columns.  These are instance methods on the Builder so
    //    that `$query->whereBrandId(42)` resolves.
    if let Some(model_class) = class_loader(model_arg) {
        let existing = lowercase_method_names(&result.methods);
        let where_methods = build_where_property_methods_for_class(&model_class, &existing);
        for mut method in where_methods {
            if !result
                .methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(&method.name))
            {
                // These are built fresh rather than interned, so the
                // forwarded return is applied in place.
                if let Some(forwarded) = forwarded
                    && let Some(ref ret) = method.return_type
                    && let Some(rewritten) =
                        forwarded_builder_return(ret, &forwarded.ty, class_loader)
                {
                    method.return_type = Some(rewritten);
                }
                result.methods.push(Arc::new(method));
            }
        }
    }
}

/// Rewrite an injected method's Builder-typed return to the
/// `ForwardsCalls` decorator it is being grafted onto, sharing the
/// rewritten copy through the interning store.
///
/// Returns `method` untouched when nothing forwards (the methods are
/// landing on a Builder) or when the return type is not Builder-typed
/// (`restoreOrCreate()` returns the model, `count()` returns `int`, and
/// both are returned as-is by `forwardDecoratedCallTo`).
fn forward_builder_return(
    method: &Arc<MethodInfo>,
    forwarded: Option<&ForwardedSelf>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Arc<MethodInfo> {
    let Some(forwarded) = forwarded else {
        return Arc::clone(method);
    };
    let Some(rewritten) = method
        .return_type
        .as_ref()
        .and_then(|ret| forwarded_builder_return(ret, &forwarded.ty, class_loader))
    else {
        return Arc::clone(method);
    };

    // The rewrite is identified by the decorator type alone: the origin
    // already carries the model-specific substitution, so every relation
    // variant of the same model and decorator shares one allocation.
    let fp = crate::virtual_members::TransformFingerprint::new(
        None,
        Some(forwarded.fp_key.as_str()),
        crate::virtual_members::cache::transform_flags::FORWARDS_CALLS_SELF,
    );
    crate::virtual_members::intern_transformed_method(method, fp, || {
        let mut m = (**method).clone();
        m.return_type = Some(rewritten);
        m
    })
}

/// Map a Builder-typed return to `self_ty`, preserving the surrounding
/// nullable / union structure.
///
/// Returns `None` when the type names nothing that forwards as the
/// builder, so callers can keep the original allocation.
fn forwarded_builder_return(
    ret: &PhpType,
    self_ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    use crate::php_type::TypeKind;

    match ret.kind() {
        TypeKind::Nullable(inner) => {
            forwarded_builder_return(inner, self_ty, class_loader).map(PhpType::nullable)
        }
        TypeKind::Union(members) => {
            let mut rewritten = false;
            let mapped: Vec<PhpType> = members
                .iter()
                .map(
                    |m| match forwarded_builder_return(m, self_ty, class_loader) {
                        Some(new) => {
                            rewritten = true;
                            new
                        }
                        None => m.clone(),
                    },
                )
                .collect();
            rewritten.then(|| PhpType::union(mapped))
        }
        // `base_name` filters out scalars and the `self`/`static`/`$this`
        // keywords, so only class-named returns reach the lookup.
        _ => ret
            .base_name()
            .is_some_and(|name| forwards_as_builder(name, class_loader))
            .then(|| self_ty.clone()),
    }
}

/// Whether a return type naming `class_name` is returned as the query
/// builder at runtime, and so comes back out of
/// `forwardDecoratedCallTo` as the decorator.
///
/// Covers the Eloquent Builder itself and any subclass of it, which is
/// what a model with a custom builder (`newEloquentBuilder()`) returns
/// from its scopes.
fn forwards_as_builder(
    class_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let stripped = class_name.strip_prefix('\\').unwrap_or(class_name);
    if stripped == ELOQUENT_BUILDER_FQN {
        return true;
    }
    class_loader(stripped).is_some_and(|cls| extends_eloquent_builder(&cls, class_loader))
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
    forwarded: Option<&ForwardedSelf>,
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
    let model_type = PhpType::named(atom(model_name));
    let subs = self_ref_subs(model_type);

    // The transform depends only on the model (via `subs`), so the
    // forwarded copies are interned and shared by every Builder and
    // relation variant of the same model.
    let fp = crate::virtual_members::TransformFingerprint::new(
        Some(&subs),
        None,
        crate::virtual_members::cache::transform_flags::FORWARD_AS_INSTANCE,
    );

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

        let transformed = crate::virtual_members::intern_transformed_method(method, fp, || {
            let mut transformed = (**method).clone();
            transformed.is_static = false;

            // Substitute self-referencing return types.
            if let Some(ref mut ret) = transformed.return_type {
                *ret = ret.substitute(&subs);
            }

            transformed
        });
        builder.methods.push(forward_builder_return(
            &transformed,
            forwarded,
            class_loader,
        ));
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
