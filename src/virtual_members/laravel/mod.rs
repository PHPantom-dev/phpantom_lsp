//! Laravel Eloquent Model virtual member provider.
//!
//! Synthesizes virtual members for classes that extend
//! `Illuminate\Database\Eloquent\Model`.  This is the highest-priority
//! virtual member provider: its contributions beat `@method` /
//! `@property` / `@mixin` members (PHPDocProvider).
//!
//! Currently implements:
//!
//! - **Relationship properties.** Methods returning a known Eloquent
//!   relationship type (e.g. `HasOne`, `HasMany`, `BelongsTo`) produce
//!   a virtual property with the same name.  The property type is
//!   inferred from the relationship's generic parameters (Larastan-style
//!   `@return HasMany<Post, $this>` annotations) or, as a fallback,
//!   from the first `::class` argument in the method body text.
//!
//! - **Scope methods.** Methods whose name starts with `scope` (e.g.
//!   `scopeActive`, `scopeVerified`) produce a virtual method with the
//!   `scope` prefix stripped and the first letter lowercased (e.g.
//!   `active`, `verified`).  Methods decorated with `#[Scope]`
//!   (Laravel 11+) are also recognized: their own name is used
//!   directly as the public-facing scope name (e.g.
//!   `#[Scope] protected function active()` becomes `active()`).
//!   The first `$query` parameter is removed.
//!   Scope methods are available as both static and instance methods
//!   so they resolve for `User::active()` and `$user->active()`.
//!
//! - **Builder-as-static forwarding.** Laravel's `Model::__callStatic()`
//!   forwards static calls to `static::query()`, which returns an
//!   Eloquent Builder.  This provider loads
//!   `\Illuminate\Database\Eloquent\Builder`, fully resolves it
//!   (including its `@mixin` on `Query\Builder`), and presents its
//!   public instance methods as static virtual methods on the model.
//!   Return types are mapped so that `static`/`$this`/`self` resolve
//!   to `Builder<ConcreteModel>` (the chain continues on the builder)
//!   and template parameters like `TModel` resolve to the concrete
//!   model class.  This makes `User::where(...)->orderBy(...)->get()`
//!   resolve end-to-end.
//!
//! - **Cast properties.** Entries in the `$casts` property array or
//!   `casts()` method body produce typed virtual properties.  Cast type
//!   strings are mapped to PHP types (e.g. `datetime` → `\Carbon\Carbon`,
//!   `boolean` → `bool`, `decimal:2` → `float`).  Custom cast classes
//!   are resolved by loading the class and inspecting the `get()`
//!   method's return type.  When the `get()` method has no return type,
//!   the resolver falls back to the first generic argument from an
//!   `@implements CastsAttributes<TGet, TSet>` annotation on the cast
//!   class.  Enum casts resolve to the enum class itself.  Classes
//!   implementing `Castable` also resolve to themselves.  A `:argument`
//!   suffix (e.g. `Address::class.':nullable'`) is stripped before
//!   resolution.
//!
//! - **Attribute default properties.** Entries in the `$attributes`
//!   property array produce typed virtual properties as a fallback.
//!   Types are inferred from the literal default values: strings,
//!   booleans, integers, floats, `null`, and arrays.  Columns that
//!   already have a `$casts` entry are skipped, so casts always take
//!   priority.
//!
//! - **Column name properties.** Column names from `$fillable`,
//!   `$guarded`, `$hidden`, and `$appends` produce `mixed`-typed
//!   virtual properties as a last-resort fallback.  Columns already
//!   covered by `$casts` or `$attributes` are skipped.
//!
//! - **`where{PropertyName}()` dynamic methods.** Laravel's
//!   `Builder::__call()` translates calls like `whereBrandId($value)`
//!   into `where('brand_id', $value)`.  For each known column on the
//!   model (from all property sources: `$casts`, `$attributes`,
//!   `$fillable`/`$guarded`/`$hidden`/`$appends`, `$dates`, timestamps,
//!   relationship `*_count` properties, `@property` annotations, and
//!   accessor-derived properties), a virtual `where{StudlyCase}()`
//!   method is synthesized.  The method accepts a `mixed` value
//!   parameter and returns `Builder<ConcreteModel>`.  These methods
//!   appear as both instance methods on the Builder (for chaining:
//!   `$query->whereBrandId(42)`) and static methods on the model
//!   (for `User::whereName('Alice')`).

mod accessors;
mod builder;
mod casts;
mod config_keys;
mod env_vars;
mod factory;
mod helpers;
pub(crate) mod patches;
mod relationships;
mod route_names;
mod scopes;
mod trans_keys;
mod view_names;
mod where_property;

pub(crate) use config_keys::find_config_references;
pub(crate) use config_keys::{
    find_all_config_references, resolve_config_key_declaration,
    resolve_config_key_definition_fallback,
};
pub(crate) use env_vars::resolve_env_definition;

/// Unified go-to-definition entry point for all Laravel string-key spans.
///
/// Dispatches on [`crate::symbol_map::LaravelStringKind`] so callers in
/// `definition/resolve.rs` only need one import and one call site.  Adding a
/// new Laravel navigation feature only requires a new match arm here, not a
/// new `pub(crate) use` in this file.
pub(crate) fn resolve_laravel_string_key(
    backend: &crate::Backend,
    kind: &crate::symbol_map::LaravelStringKind,
    key: &str,
) -> Vec<tower_lsp::lsp_types::Location> {
    use crate::symbol_map::LaravelStringKind;
    match kind {
        LaravelStringKind::Config => resolve_config_key_declaration(backend, key)
            .into_iter()
            .collect(),
        LaravelStringKind::View => view_names::resolve_view_definitions(backend, key),
        LaravelStringKind::Route => route_names::resolve_route_definitions(backend, key),
        LaravelStringKind::Trans => trans_keys::resolve_trans_definitions(backend, key),
    }
}

/// Unified find-references entry point for all Laravel string-key spans.
///
/// Dispatches on [`crate::symbol_map::LaravelStringKind`] — see
/// [`resolve_laravel_string_key`] for the same rationale.
pub(crate) fn find_laravel_string_key_references(
    backend: &crate::Backend,
    kind: &crate::symbol_map::LaravelStringKind,
    key: &str,
    snapshot: &[(String, std::sync::Arc<crate::symbol_map::SymbolMap>)],
    include_declaration: bool,
) -> Vec<tower_lsp::lsp_types::Location> {
    use crate::symbol_map::LaravelStringKind;
    let mut locations = match kind {
        LaravelStringKind::Config => {
            find_all_config_references(backend, key, snapshot, include_declaration)
        }
        LaravelStringKind::View | LaravelStringKind::Route | LaravelStringKind::Trans => {
            find_string_key_usages(kind, key, backend, snapshot)
        }
    };

    if include_declaration && kind != &LaravelStringKind::Config {
        for decl in resolve_laravel_string_key(backend, kind, key) {
            crate::util::push_unique_location(
                &mut locations,
                &decl.uri,
                decl.range.start,
                decl.range.end,
            );
        }
    }

    locations
}

/// Scan pre-built [`crate::symbol_map::SymbolMap`] spans for all call sites
/// matching `kind` + `key` — zero file re-parses, O(total spans) memory walk.
fn find_string_key_usages(
    kind: &crate::symbol_map::LaravelStringKind,
    key: &str,
    backend: &crate::Backend,
    snapshot: &[(String, std::sync::Arc<crate::symbol_map::SymbolMap>)],
) -> Vec<tower_lsp::lsp_types::Location> {
    use crate::symbol_map::SymbolKind;
    use crate::util::{offset_to_position, push_unique_location};
    use tower_lsp::lsp_types::Url;

    let mut locations = Vec::new();
    for (file_uri, symbol_map) in snapshot {
        // First pass: check if this file even has ANY LaravelStringKey matches.
        // This avoids reading file content from disk for thousands of unrelated files.
        let has_match = symbol_map.spans.iter().any(|span| {
            if let SymbolKind::LaravelStringKey {
                kind: span_kind,
                key: span_key,
            } = &span.kind
            {
                span_kind == kind && span_key == key
            } else {
                false
            }
        });

        if !has_match {
            continue;
        }

        let Ok(parsed_uri) = Url::parse(file_uri) else {
            continue;
        };
        let Some(content) = backend.get_file_content_arc(file_uri) else {
            continue;
        };
        for span in &symbol_map.spans {
            if let SymbolKind::LaravelStringKey {
                kind: span_kind,
                key: span_key,
            } = &span.kind
                && span_kind == kind
                && span_key == key
            {
                let start = offset_to_position(&content, span.start as usize);
                let end = offset_to_position(&content, span.end as usize);
                push_unique_location(&mut locations, &parsed_uri, start, end);
            }
        }
    }
    locations
}

pub use helpers::extends_eloquent_model;
pub(crate) use helpers::{accessor_method_candidates, camel_to_snake};

pub(crate) use accessors::is_accessor_method;
use accessors::{
    extract_modern_accessor_type, is_legacy_accessor, is_modern_accessor,
    legacy_accessor_property_name,
};
pub(crate) use where_property::where_property_method_to_column;

pub(crate) use relationships::classify_relationship_typed;
pub(crate) use relationships::count_property_to_relationship_method;
pub use relationships::infer_relationship_from_body;
pub(crate) use relationships::{RELATION_QUERY_METHODS, resolve_relation_chain};
use relationships::{
    RelationshipKind, build_property_type, count_property_name, extract_related_type_typed,
};

pub use scopes::build_scope_methods_for_builder;
use scopes::{build_scope_methods, is_scope_method};
use where_property::{build_where_property_methods_for_class, lowercase_method_names};

use std::collections::HashMap;
use std::sync::Arc;

use builder::build_builder_forwarded_methods;
use casts::cast_type_to_php_type;
pub use factory::LaravelFactoryProvider;
pub(crate) use factory::{factory_to_model_fqn, model_to_factory_fqn};

use crate::php_type::PhpType;
use crate::types::{ClassInfo, PropertyInfo};

use super::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};

/// The fully-qualified name of the Eloquent base model.
pub(crate) const ELOQUENT_MODEL_FQN: &str = "Illuminate\\Database\\Eloquent\\Model";

/// The fully-qualified name of the Eloquent Builder class.
pub const ELOQUENT_BUILDER_FQN: &str = "Illuminate\\Database\\Eloquent\\Builder";

/// Build a substitution map that replaces `static`, `$this`, and `self`
/// with the given type.
///
/// This is used across multiple Laravel virtual member providers
/// (builder forwarding, model virtual methods, scope methods) to
/// resolve self-referencing return types to concrete model or builder
/// types.
pub(super) fn self_ref_subs(ty: PhpType) -> HashMap<String, PhpType> {
    HashMap::from([
        ("static".to_owned(), ty.clone()),
        ("$this".to_owned(), ty.clone()),
        ("self".to_owned(), ty),
    ])
}

// ─── Type-resolution helpers ────────────────────────────────────────────────
//
// Called from `completion/resolver.rs` (`type_hint_to_classes_depth`) to
// apply Eloquent-specific post-processing after a class has been resolved
// and generic substitution applied.  Keeping the framework logic here
// rather than inline in the generic resolver avoids coupling the type
// engine to Laravel conventions.

/// Swap a resolved Eloquent Collection to a model's custom collection.
///
/// When the resolved class is `Illuminate\Database\Eloquent\Collection`
/// and one of the generic type arguments is a model with a
/// `custom_collection` declared (via `#[CollectedBy]` or
/// `@use HasCollection<X>`), returns the custom collection class
/// instead.  This handles the common chain pattern:
///
/// ```php
/// Model::where(...)->get()  // returns Collection<int, TModel>
/// ```
///
/// where `TModel` has been substituted to the concrete model and the
/// model declares a custom collection like `ProductCollection`.
///
/// Returns `None` when the class is not the Eloquent Collection, has no
/// generic args, or the model does not declare a custom collection.
pub(crate) fn try_swap_custom_collection(
    cls: ClassInfo,
    base_fqn: &str,
    generic_args: &[PhpType],
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> ClassInfo {
    if base_fqn != crate::types::ELOQUENT_COLLECTION_FQN || generic_args.is_empty() {
        return cls;
    }

    // The last generic arg is typically the model type.
    let model_name = match generic_args.last().unwrap().base_name() {
        Some(name) => name.to_string(),
        None => return cls,
    };
    let model_class = find_class_in(all_classes, &model_name)
        .cloned()
        .or_else(|| class_loader(&model_name).map(Arc::unwrap_or_clone));

    if let Some(ref mc) = model_class
        && let Some(coll_type) = mc.laravel().and_then(|l| l.custom_collection.as_ref())
    {
        let coll_name = coll_type.to_string();
        find_class_in(all_classes, &coll_name)
            .cloned()
            .or_else(|| class_loader(&coll_name).map(Arc::unwrap_or_clone))
            .unwrap_or(cls)
    } else {
        cls
    }
}

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
    use crate::php_type::PhpType;

    let model_class = match class_loader(model_name) {
        Some(c) => c,
        None => return,
    };

    if !extends_eloquent_model(&model_class, class_loader) {
        return;
    }

    // Resolve the model fully so that @method tags from traits and
    // parent classes are included.
    let resolved_model =
        crate::virtual_members::resolve_class_fully_maybe_cached(&model_class, class_loader, cache);

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
        || helpers::extends_eloquent_builder(cls, class_loader)
}

/// Find a class in a slice by name (short or FQN).
///
/// Minimal local lookup used by the collection-swap helper.  Prefers
/// namespace-aware matching when the name contains backslashes.
fn find_class_in<'a>(all_classes: &'a [Arc<ClassInfo>], name: &str) -> Option<&'a ClassInfo> {
    let short = name.rsplit('\\').next().unwrap_or(name);

    if name.contains('\\') {
        let expected_ns = name.rsplit_once('\\').map(|(ns, _)| ns);
        all_classes
            .iter()
            .find(|c| c.name == short && c.file_namespace.as_deref() == expected_ns)
            .map(|c| c.as_ref())
    } else {
        all_classes
            .iter()
            .find(|c| c.name == short)
            .map(|c| c.as_ref())
    }
}

/// Virtual member provider for Laravel Eloquent models.
///
/// When a class extends `Illuminate\Database\Eloquent\Model` (directly
/// or through an intermediate parent), this provider scans its methods
/// for Eloquent relationship return types and synthesizes virtual
/// properties for each one.
///
/// For example, a method `posts()` returning `HasMany<Post, $this>`
/// produces a virtual property `$posts` with type
/// `\Illuminate\Database\Eloquent\Collection<Post>`.
pub struct LaravelModelProvider;

/// Pre-built `Carbon\Carbon` type used for date-related virtual properties.
fn carbon_type() -> PhpType {
    PhpType::Named("Carbon\\Carbon".to_owned())
}

impl VirtualMemberProvider for LaravelModelProvider {
    /// Returns `true` if the class extends `Illuminate\Database\Eloquent\Model`.
    fn applies_to(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        extends_eloquent_model(class, class_loader)
    }

    /// Scan the class's methods for Eloquent relationship return types,
    /// scope methods, Builder-as-static forwarded methods, `$casts`
    /// definitions, `$attributes` defaults, and `$fillable`/`$guarded`/
    /// `$hidden`/`$appends` column names.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut seen_props: std::collections::HashSet<String> = std::collections::HashSet::new();

        // ── Cast properties ─────────────────────────────────────────
        if let Some(laravel) = class.laravel() {
            for (column, cast_type) in &laravel.casts_definitions {
                let php_type = cast_type_to_php_type(cast_type, class_loader);
                seen_props.insert(column.clone());
                properties.push(PropertyInfo::virtual_property_typed(
                    column,
                    Some(&php_type),
                ));
            }

            // ── $dates properties (deprecated, lower priority than $casts) ──
            // Columns in `$dates` are typed as Carbon\Carbon unless already
            // covered by an explicit `$casts` entry.
            for column in &laravel.dates_definitions {
                if !seen_props.insert(column.clone()) {
                    continue;
                }
                properties.push(PropertyInfo::virtual_property_typed(
                    column,
                    Some(&carbon_type()),
                ));
            }

            // ── Attribute default properties (fallback) ─────────────
            // Only add properties for columns not already covered by $casts
            // or $dates.
            for (column, php_type) in &laravel.attributes_definitions {
                if !seen_props.insert(column.clone()) {
                    continue;
                }
                properties.push(PropertyInfo::virtual_property_typed(column, Some(php_type)));
            }

            // ── Column name properties (last-resort fallback) ───────
            // $fillable, $guarded, $hidden, and $appends provide column
            // names without type info.  Only add those not already covered.
            for column in &laravel.column_names {
                if !seen_props.insert(column.clone()) {
                    continue;
                }
                properties.push(PropertyInfo::virtual_property_typed(
                    column,
                    Some(&PhpType::mixed()),
                ));
            }

            // ── Timestamp properties ────────────────────────────────
            // When $timestamps is true (the default), synthesize
            // created_at and updated_at as Carbon\Carbon.  The column
            // names can be overridden via CREATED_AT / UPDATED_AT
            // constants, and either can be disabled by setting the
            // constant to null.
            let timestamps_enabled = laravel.timestamps.unwrap_or(true);
            if timestamps_enabled {
                let created_col = match &laravel.created_at_name {
                    Some(Some(name)) => Some(name.as_str()),
                    Some(None) => None,         // explicitly null
                    None => Some("created_at"), // default
                };
                let updated_col = match &laravel.updated_at_name {
                    Some(Some(name)) => Some(name.as_str()),
                    Some(None) => None,         // explicitly null
                    None => Some("updated_at"), // default
                };
                for col in [created_col, updated_col].into_iter().flatten() {
                    if seen_props.insert(col.to_string()) {
                        properties.push(PropertyInfo::virtual_property_typed(
                            col,
                            Some(&carbon_type()),
                        ));
                    }
                }
            }
        }

        for method in &class.methods {
            // ── Scope methods ───────────────────────────────────────
            if is_scope_method(method) {
                // Skip `#[Scope]`-attributed methods that also use
                // the `scopeX` prefix — the attribute takes priority
                // and the name is used as-is (no prefix stripping).
                let [instance_method, static_method] = build_scope_methods(method);
                methods.push(instance_method);
                methods.push(static_method);
                continue;
            }

            // ── Legacy accessors (getXAttribute) ────────────────────
            if is_legacy_accessor(method) {
                let prop_name = legacy_accessor_property_name(&method.name);
                properties.push(PropertyInfo {
                    deprecation_message: method.deprecation_message.clone(),
                    ..PropertyInfo::virtual_property_typed(&prop_name, method.return_type.as_ref())
                });
                continue;
            }

            // ── Modern accessors (Laravel 9+ Attribute casts) ───────
            if is_modern_accessor(method) {
                let prop_name = camel_to_snake(&method.name);
                let accessor_type = extract_modern_accessor_type(method);
                properties.push(PropertyInfo {
                    deprecation_message: method.deprecation_message.clone(),
                    ..PropertyInfo::virtual_property_typed(&prop_name, Some(&accessor_type))
                });
                continue;
            }

            // ── Relationship properties ─────────────────────────────
            let return_type = match method.return_type.as_ref() {
                Some(rt) => rt,
                None => continue,
            };

            let kind = match classify_relationship_typed(return_type) {
                Some(k) => k,
                None => continue,
            };

            let related_type = extract_related_type_typed(return_type);

            // For collection relationships, use the *related* model's
            // custom_collection, not the owning model's.  For example,
            // if Product has `#[CollectedBy(ProductCollection)]` and
            // Review has `#[CollectedBy(ReviewCollection)]`, then
            // `Product::reviews()` returning `HasMany<Review, $this>`
            // should produce `ReviewCollection<Review>`, not
            // `ProductCollection<Review>`.
            let custom_collection = if kind == RelationshipKind::Collection {
                related_type
                    .and_then(|t| t.base_name().and_then(class_loader))
                    .and_then(|related_class| {
                        related_class
                            .laravel
                            .as_ref()
                            .and_then(|l| l.custom_collection.as_ref().map(|c| c.to_string()))
                    })
            } else {
                None
            };

            let type_hint = build_property_type(kind, related_type, custom_collection.as_deref());

            if let Some(ref th) = type_hint {
                properties.push(PropertyInfo::virtual_property_typed(&method.name, Some(th)));
            }
        }

        // ── Relationship count properties (`*_count`) ───────────────
        // `withCount`/`loadCount` is one of the most common Eloquent
        // patterns.  For each relationship method, synthesize a
        // `{snake_name}_count` property typed as `int`.  Skip if a
        // property with that name already exists (e.g. from an explicit
        // `@property` tag).
        for method in &class.methods {
            let return_type = match method.return_type.as_ref() {
                Some(rt) => rt,
                None => continue,
            };
            if classify_relationship_typed(return_type).is_none() {
                continue;
            }
            let count_name = count_property_name(&method.name);
            if !seen_props.insert(count_name.clone()) {
                continue;
            }
            properties.push(PropertyInfo::virtual_property_typed(
                &count_name,
                Some(&PhpType::int()),
            ));
        }

        // ── Builder-as-static forwarding ────────────────────────────
        let forwarded = build_builder_forwarded_methods(class, class_loader, cache);
        methods.extend(forwarded);

        // ── where{PropertyName}() static forwarding ─────────────────
        // Laravel's Model::__callStatic() delegates to Builder, which
        // handles where{Column}() calls.  Synthesize these as static
        // methods on the model so that User::whereName('Alice') resolves.
        let existing = lowercase_method_names(&methods);
        let where_static = build_where_property_methods_for_class(class, &existing);
        for mut m in where_static {
            m.is_static = true;
            methods.push(m);
        }

        VirtualMembers {
            methods,
            properties,
            constants: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
